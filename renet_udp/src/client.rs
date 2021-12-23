use renet::channel::reliable::ReliableChannelConfig;
use renet::error::{DisconnectionReason, RenetError};
use renet::packet::Payload;
use renet::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};

use log::debug;

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use crate::RenetUdpError;

pub struct UdpClient {
    id: SocketAddr,
    socket: UdpSocket,
    connection: RemoteConnection,
    server_addr: SocketAddr,
}

impl UdpClient {
    pub fn new(
        socket: UdpSocket,
        server_addr: SocketAddr,
        connection_config: ConnectionConfig,
        reliable_channels_config: Vec<ReliableChannelConfig>,
    ) -> Result<Self, std::io::Error> {
        socket.set_nonblocking(true)?;
        let id = socket.local_addr()?;
        let connection = RemoteConnection::new(connection_config, reliable_channels_config);

        Ok(Self {
            socket,
            id,
            connection,
            server_addr,
        })
    }

    pub fn id(&self) -> SocketAddr {
        self.id
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    pub fn connection_error(&self) -> Option<DisconnectionReason> {
        self.connection.disconnected()
    }

    pub fn disconnect(&mut self) {
        // TODO: get packets disconnect packets to send
        // Resend disconnect packets x times based on an constant (or config)
        // yojimbo uses 10 disconnect packets.
        self.connection.disconnect();
    }

    pub fn send_reliable_message<ChannelId: Into<u8>>(
        &mut self,
        channel_id: ChannelId,
        message: Payload,
    ) {
        self.connection
            .send_reliable_message(channel_id.into(), message);
    }

    pub fn send_unreliable_message(&mut self, message: Payload) {
        self.connection.send_unreliable_message(message);
    }

    pub fn send_block_message(&mut self, message: Payload) {
        self.connection.send_block_message(message);
    }

    pub fn receive_reliable_message(&mut self, channel_id: u8) -> Option<Payload> {
        self.connection.receive_reliable_message(channel_id)
    }

    pub fn receive_unreliable_message(&mut self) -> Option<Payload> {
        self.connection.receive_unreliable_message()
    }

    pub fn receive_block_message(&mut self) -> Option<Payload> {
        self.connection.receive_block_message()
    }

    pub fn network_info(&self) -> &NetworkInfo {
        self.connection.network_info()
    }

    pub fn send_packets(&mut self) -> Result<(), RenetUdpError> {
        let packets = self.connection.get_packets_to_send()?;
        for packet in packets.into_iter() {
            self.socket.send_to(&packet, self.server_addr)?;
        }
        Ok(())
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), RenetUdpError> {
        if let Some(connection_error) = self.connection_error() {
            return Err(RenetError::ConnectionError(connection_error).into());
        }
        self.connection.advance_time(duration);

        let mut buffer = vec![0; self.connection.config.max_packet_size as usize];
        loop {
            let packet = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        &buffer[..len]
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(RenetUdpError::IOError(e)),
            };

            self.connection.process_packet(packet)?;
        }

        self.connection.update()?;
        Ok(())
    }
}
