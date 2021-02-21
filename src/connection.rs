use crate::channel::{Channel, ChannelPacketData};
use crate::endpoint::Endpoint;
use crate::error::RenetError;
use crate::packet::PacketType;
use crate::protocol::SecurityService;
use crate::Timer;

use log::{error, debug};

use std::net::{SocketAddr, UdpSocket};
use std::collections::HashMap;

pub type ClientId = u64;

pub struct Connection<S> {
    pub(crate) endpoint: Endpoint,
    pub(crate) addr: SocketAddr,
    channels: HashMap<u8, Box<dyn Channel>>,
    security_service: S,
    heartbeat_timer: Timer,
    timeout_timer: Timer,
}

impl<S: SecurityService> Connection<S> {
    pub fn new(
        server_addr: SocketAddr,
        endpoint: Endpoint,
        security_service: S,
    ) -> Self {
        let timeout_timer = Timer::new(endpoint.config().timeout_duration);
        let heartbeat_timer = Timer::new(endpoint.config().heartbeat_time);
        Self {
            endpoint,
            channels: HashMap::new(),
            addr: server_addr,
            security_service,
            timeout_timer,
            heartbeat_timer,
        }
    }

    pub fn add_channel(&mut self, channel_id: u8, channel: Box<dyn Channel>) {
        self.channels.insert(channel_id, channel);
    }

    pub fn has_timed_out(&mut self) -> bool {
        self.timeout_timer.is_finished()
    }

    pub fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) {
        let channel = self
            .channels
            .get_mut(&channel_id)
            .expect("Sending message to invalid channel");
        channel.send_message(message);
    }

    pub fn process_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        self.timeout_timer.reset();
        let payload = self.security_service.ss_unwrap(payload)?;
        let payload = self.endpoint.process_payload(&payload)?;
        for ack in self.endpoint.get_acks().iter() {
            for channel in self.channels.values_mut() {
                channel.process_ack(*ack);
            }
        }
        self.endpoint.reset_acks();

        let payload = match payload {
            Some(payload) => payload,
            None => return Ok(()),
        };

        let mut channel_packets = match bincode::deserialize::<Vec<ChannelPacketData>>(&payload) {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to deserialize ChannelPacketData: {:?}", e);
                // TODO: remove bincode and serde, update serialize errors
                return Err(RenetError::SerializationFailed);
            }
        };

        for channel_packet_data in channel_packets.drain(..) {
            let channel = match self.channels.get_mut(&channel_packet_data.channel_id) {
                Some(c) => c,
                None => {
                    error!(
                        "Received channel packet with invalid id: {:?}",
                        channel_packet_data.channel_id
                    );
                    continue;
                }
            };
            channel.process_messages(channel_packet_data.messages);
        }

        Ok(())
    }

    pub fn send_payload(&mut self, payload: &[u8], socket: &UdpSocket) -> Result<(), RenetError> {
        let reliable_packets = self.endpoint.generate_packets(payload)?;
        for reliable_packet in reliable_packets.iter() {
            let payload = self.security_service.ss_wrap(&reliable_packet).unwrap();
            socket.send_to(&payload, self.addr)?;
        }
        Ok(())
    }

    pub fn send_packets(&mut self, socket: &UdpSocket) -> Result<(), RenetError> {
        if let Some(payload) = self.get_packet()? {
            self.heartbeat_timer.reset();
            self.send_payload(&payload, socket).unwrap();
        } else if self.heartbeat_timer.is_finished() {
            self.heartbeat_timer.reset();
            let packet = self.endpoint.build_heartbeat_packet().unwrap();
            let payload = self.security_service.ss_wrap(&packet).unwrap();
            socket.send_to(&payload, self.addr).unwrap();
        }
        Ok(())
    }

    pub fn get_packet(&mut self) -> Result<Option<Box<[u8]>>, RenetError> {
        let sequence = self.endpoint.sequence();
        let mut channel_packets: Vec<ChannelPacketData> = vec![];
        for (channel_id, channel) in self.channels.iter_mut() {
            let messages = channel.get_messages_to_send(
                Some(self.endpoint.config().max_packet_size as u32),
                sequence,
            );
            if let Some(messages) = messages {
                debug!("Sending {} messages.", messages.len());
                let packet_data = ChannelPacketData::new(messages, *channel_id);
                channel_packets.push(packet_data);
            }
        }

        if channel_packets.is_empty() {
            return Ok(None);
        }

        let payload = match bincode::serialize(&channel_packets) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to serialize Vec<ChannelPacketData>: {:?}", e);
                return Err(RenetError::SerializationFailed);
            }
        };

        Ok(Some(payload.into_boxed_slice()))
    }

    pub fn receive_message_from_channel(&mut self, channel_id: u8) -> Option<Box<[u8]>> {
        let channel = match self.channels.get_mut(&channel_id) {
            Some(c) => c,
            None => {
                error!(
                    "Tried to receive message from invalid channel {}.",
                    channel_id
                );
                return None;
            }
        };

        channel.receive_message()
    }

    pub fn receive_all_messages_from_channel(&mut self, channel_id: u8) -> Vec<Box<[u8]>> {
        let mut messages = vec![];
        let channel = match self.channels.get_mut(&channel_id) {
            Some(c) => c,
            None => {
                error!(
                    "Tried to receive message from invalid channel {}.",
                    channel_id
                );
                return messages;
            }
        };

        while let Some(message) = channel.receive_message() {
            messages.push(message);
        }

        messages
    }
}
