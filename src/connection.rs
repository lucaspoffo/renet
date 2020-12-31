use crate::channel::{Channel, ChannelConfig, ChannelPacketData};
use crate::client::ClientConnected;
use crate::endpoint::{Config, Endpoint, NetworkInfo};
use crate::error::RenetError;
use crate::server::{Server, ServerConfig};
use crate::protocol::{SecurityService, AuthenticationProtocol};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use log::{debug, error, info};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;

pub struct Connection {
    pub endpoint: Endpoint,
    channels: HashMap<u8, Box<dyn Channel>>,
    addr: SocketAddr,
    security_service: Box<dyn SecurityService>
}

impl Connection {
    pub fn new(server_addr: SocketAddr, endpoint: Endpoint, security_service: Box<dyn SecurityService>) -> Self {
        Self {
            endpoint,
            channels: HashMap::new(),
            addr: server_addr,
            security_service
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn add_channel(&mut self, channel_id: u8, mut channel: Box<dyn Channel>) {
        channel.set_id(channel_id);
        self.channels.insert(channel_id, channel);
    }

    pub fn update_channels_current_time(&mut self, current_time: Instant) {
        for channel in self.channels.values_mut() {
            channel.update_current_time(current_time.clone());
        }
    }

    pub fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) {
        let channel = self
            .channels
            .get_mut(&channel_id)
            .expect("Sending message to invalid channel");
        channel.send_message(message);
    }

    pub fn process_payload(&mut self, payload: Box<[u8]>) -> Result<(), RenetError> {
        let payload = self.security_service.ss_unwrap(payload)?;
        let payload = match self.endpoint.process_payload(&payload)? {
            Some(payload) => payload,
            None => return Ok(())
        };

        let channel_packets = match bincode::deserialize::<Vec<ChannelPacketData>>(&payload) {
            Ok(x) => x,
            Err(e) => {
                error!("Failed to deserialize ChannelPacketData: {:?}", e);
                // TODO: remove bincode and serde, update serialize errors
                return Err(RenetError::SerializationFailed);
            }
        };

        for channel_packet_data in channel_packets.iter() {
            let channel = match self.channels.get_mut(&channel_packet_data.channel_id()) {
                Some(c) => c,
                None => {
                    error!(
                        "Received channel packet with invalid id: {:?}",
                        channel_packet_data.channel_id()
                    );
                    continue;
                }
            };
            channel.process_packet_data(channel_packet_data);
        }

        for ack in self.endpoint.get_acks().iter() {
            for channel in self.channels.values_mut() {
                channel.process_ack(*ack);
            }
        }
        self.endpoint.reset_acks();
        Ok(())
    }

    pub fn send_payload(&mut self, payload: &[u8], socket: &UdpSocket) -> Result<(), RenetError> {
        let reliable_packets = self.endpoint.generate_packets(payload)?;
        for reliable_packet in reliable_packets.iter() {
            // TODO: remove clone
            let payload = self.security_service.ss_wrap(reliable_packet.clone().into_boxed_slice())?;
            socket.send_to(&payload, self.addr)?;
        }
        Ok(())
    }

    pub fn get_packet(&mut self) -> Result<Option<Box<[u8]>>, RenetError> {
        let sequence = self.endpoint.sequence();
        let mut channel_packets: Vec<ChannelPacketData> = vec![];
        for channel in self.channels.values_mut() {
            let packet_data = channel.get_packet_data(
                Some(self.endpoint.config().max_packet_size as u32),
                sequence,
            );
            if let Some(packet_data) = packet_data {
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

        return channel.receive_message();
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

pub struct RequestConnection {
    socket: UdpSocket,
    server_addr: SocketAddr,
    id: ClientId,
    protocol: Box<dyn AuthenticationProtocol>
}

impl RequestConnection {
    pub fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        protocol: Box<dyn AuthenticationProtocol>
    ) -> Result<Self, RenetError> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            id,
            socket,
            server_addr,
            protocol
        })
    }

    fn process_payload(&mut self, payload: Box<[u8]>) -> Result<(), RenetError> {
        self.protocol.read_payload(payload)?;
        Ok(())
    }

    pub fn update(&mut self) -> Result<Option<ClientConnected>, RenetError> {
        self.process_events()?;
        
        if self.protocol.is_authenticated() {
            let config = Config::default();
            let endpoint = Endpoint::new(config);
            let security_service = self.protocol.build_security_interface();
            return Ok(Some(ClientConnected::new(
                self.id,
                self.socket.try_clone()?,
                self.server_addr,
                endpoint,
                security_service
            )));
        }

        match self.protocol.create_payload() {
            Ok(Some(payload)) => {
                info!("Sending protocol payload to server: {:?}", payload);
                self.socket.send_to(&payload, self.server_addr)?; 
            },
            Ok(None) => {},
            Err(e) => error!("Failed to create protocol payload: {:?}", e),
        }

        Ok(None)
    }

    fn process_events(&mut self) -> Result<(), RenetError> {
        // TODO: remove this buffer
        let mut buffer = vec![0u8; 1500];
        loop {
            let payload = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        buffer[..len].to_vec().into_boxed_slice()
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(RenetError::IOError(e)),
            };

            self.process_payload(payload)?;
        }
    }
}

pub struct HandleConnection {
    addr: SocketAddr,
    client_id: ClientId,
    pub(crate) protocol: Box<dyn AuthenticationProtocol>
}

impl HandleConnection {
    pub fn new(client_id: ClientId, addr: SocketAddr, protocol: Box<dyn AuthenticationProtocol>) -> Self {
        Self {
            client_id,
            addr,
            protocol
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub fn process_payload(&mut self, payload: Box<[u8]>) {
        if let Err(e) = self.protocol.read_payload(payload) {
            error!("Error reading protocol payload:\n{:?}", e);
        }
    }
}

pub type ClientId = u64;

/*
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, UdpSocket};

    #[test]
    fn encode_decode_payload() {
        let payload = vec![1, 2, 3, 4, 5].into_boxed_slice();
        let packet = ConnectionPacket::Payload(payload);
        let mut buffer = vec![0u8; packet.size()];

        packet.encode(&mut buffer).unwrap();
        let decoded_packet = ConnectionPacket::decode(&buffer).unwrap();

        assert_eq!(packet, decoded_packet);
    }

    #[test]
    fn encode_decode_connection() {
        let packet = ConnectionPacket::ConnectionRequest(1);
        let mut buffer = vec![0u8; packet.size()];

        packet.encode(&mut buffer).unwrap();
        let decoded_packet = ConnectionPacket::decode(&buffer).unwrap();

        assert_eq!(packet, decoded_packet);
    }

    #[test]
    fn request_connection_flow() {
        let socket = UdpSocket::bind("127.0.0.1:8081").unwrap();
        let server_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut request_connection = RequestConnection::new(0, socket, server_addr).unwrap();

        assert_eq!(
            ConnectionState::SendingConnectionRequest,
            request_connection.state
        );

        request_connection
            .process_packet(ConnectionPacket::Challenge)
            .unwrap();
        assert_eq!(
            ConnectionState::SendingChallengeResponse,
            request_connection.state
        );

        request_connection
            .process_packet(ConnectionPacket::HeartBeat)
            .unwrap();
        assert_eq!(ConnectionState::Accepted, request_connection.state);
    }
}
*/
