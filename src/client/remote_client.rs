use crate::channel::reliable::ReliableChannelConfig;
use crate::client::Client;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::Payload;
use crate::protocol::AuthenticationProtocol;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};

use log::debug;

use std::io;
use std::net::{SocketAddr, UdpSocket};

pub struct RemoteClient<A: AuthenticationProtocol> {
    socket: UdpSocket,
    id: A::ClientId,
    connection: RemoteConnection<A>,
}

impl<A: AuthenticationProtocol> RemoteClient<A> {
    pub fn new(
        id: A::ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        protocol: A,
        connection_config: ConnectionConfig,
        realiable_channels_config: Vec<ReliableChannelConfig>,
    ) -> Result<Self, std::io::Error> {
        socket.set_nonblocking(true)?;
        let connection = RemoteConnection::new(
            id,
            server_addr,
            connection_config,
            protocol,
            realiable_channels_config,
        );

        Ok(Self {
            socket,
            id,
            connection,
        })
    }
}

impl<A: AuthenticationProtocol> Client<A::ClientId> for RemoteClient<A> {
    fn id(&self) -> A::ClientId {
        self.id
    }

    fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        self.connection.disconnected()
    }

    fn disconnect(&mut self) {
        self.connection.disconnect(&self.socket);
    }

    fn send_reliable_message(&mut self, channel_id: u8, message: Payload) {
        self.connection.send_reliable_message(channel_id, message);
    }

    fn send_unreliable_message(&mut self, message: Payload) {
        self.connection.send_unreliable_message(message);
    }

    fn send_block_message(&mut self, message: Payload) {
        self.connection.send_block_message(message);
    }

    fn receive_message(&mut self) -> Option<Payload> {
        self.connection.receive_message()
    }

    fn network_info(&self) -> &NetworkInfo {
        self.connection.network_info()
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        self.connection.send_packets(&self.socket)?;
        Ok(())
    }

    fn update(&mut self) -> Result<(), RenetError> {
        if let Some(connection_error) = self.connection_error() {
            return Err(RenetError::ConnectionError(connection_error));
        }

        let mut buffer = vec![0; self.connection.config.max_packet_size as usize];
        loop {
            let payload = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == *self.connection.addr() {
                        &buffer[..len]
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(RenetError::IOError(e)),
            };

            self.connection.process_payload(payload)?;
        }

        self.connection.update()
    }
}
