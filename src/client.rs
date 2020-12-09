use crate::channel::Channel;
use crate::endpoint::{Endpoint, NetworkInfo};
use crate::error::RenetError;
use crate::connection::{ClientId, Connection, ConnectionPacket, ConnectionError};
use log::{debug, error};
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;
use std::io;

pub struct ClientConnected {
    socket: UdpSocket,
    id: ClientId,
    connection: Connection,
}

impl ClientConnected {
    pub fn new(id: ClientId, socket: UdpSocket, server_addr: SocketAddr, endpoint: Endpoint) -> Self {
        Self {
            id,
            socket,
            connection: Connection::new(server_addr, endpoint),
        }
    }

    pub fn add_channel(&mut self, channel_id: u8, channel: Box<dyn Channel>) {
        self.connection.add_channel(channel_id, channel);
    }

    pub fn id(&self) -> ClientId {
        self.id
    }

    pub fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) {
        self.connection.send_message(channel_id, message);
    }

    pub fn receive_all_messages_from_channel(&mut self, channel_id: u8) -> Vec<Box<[u8]>> {
        self.connection
            .receive_all_messages_from_channel(channel_id)
    }

    pub fn network_info(&self) -> &NetworkInfo {
        self.connection.endpoint.network_info()
    }

    pub fn update_network_info(&mut self) {
        self.connection.endpoint.update_sent_bandwidth();
        self.connection.endpoint.update_received_bandwidth();
    }

    pub fn send_packets(&mut self) -> Result<(), RenetError> {
        if let Some(payload) = self.connection.get_packet()? {
            self.connection.send_payload(&payload, &self.socket)?;
        }
        Ok(())
    }

    fn process_packet(&mut self, mut packet: ConnectionPacket) {
        match packet {
            ConnectionPacket::Payload(ref mut payload) => {
                match self.connection.endpoint.process_payload(payload) {
                    Ok(Some(payload)) => {
                        self.connection.process_payload(&payload);
                    }
                    Err(e) => {
                        error!(
                            "Error in endpoint from server while processing payload:\n{:?}",
                            e
                        );
                    }
                    Ok(None) => {}
                }
            }
            ConnectionPacket::HeartBeat => {
                debug!("Received HeartBeat from the server");
            }
            _ => {
                debug!(
                    "Ignoring Packet type {} while in connected state",
                    packet.id()
                );
            }
        }
    }

    pub fn process_events(&mut self, current_time: Instant) -> Result<(), RenetError> {
        let mut buffer = vec![0u8; 1500];
        self.connection.update_channels_current_time(current_time);
        loop {
            let packet = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == *self.connection.addr() {
                        let payload = &buffer[..len];
                        ConnectionPacket::decode(payload)?
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };
            self.process_packet(packet);
        }
    }
}
