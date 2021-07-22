use crate::channel::ChannelConfig;
use crate::client::Client;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::Payload;
use crate::protocol::AuthenticationProtocol;
use crate::remote_connection::{ClientId, ConnectionConfig, NetworkInfo, RemoteConnection};

use log::debug;

use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};

pub struct RemoteClient<A: AuthenticationProtocol> {
    socket: UdpSocket,
    id: ClientId,
    connection: RemoteConnection<A>,
    buffer: Box<[u8]>,
}

impl<A: AuthenticationProtocol> RemoteClient<A> {
    pub fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
        protocol: A,
        connection_config: ConnectionConfig,
    ) -> Result<Self, std::io::Error> {
        socket.set_nonblocking(true)?;
        let buffer = vec![0; connection_config.max_packet_size].into_boxed_slice();
        let mut connection = RemoteConnection::new(id, server_addr, connection_config, protocol);

        for (channel_id, channel_config) in channels_config.iter() {
            let channel = channel_config.new_channel();
            connection.add_channel(*channel_id, channel);
        }

        Ok(Self {
            socket,
            id,
            connection,
            buffer,
        })
    }
}

impl<A: AuthenticationProtocol> Client for RemoteClient<A> {
    fn id(&self) -> ClientId {
        self.id
    }

    fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        self.connection.connection_error()
    }

    fn disconnect(&mut self) {
        self.connection.disconnect(&self.socket);
    }

    fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), RenetError> {
        self.connection.send_message(channel_id, message)
    }

    fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, RenetError> {
        self.connection.receive_message(channel_id)
    }

    fn network_info(&mut self) -> &NetworkInfo {
        self.connection.network_info()
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        self.connection.send_packets(&self.socket)?;
        Ok(())
    }

    fn update(&mut self) -> Result<(), RenetError> {
        if let Some(connection_error) = self.connection_error() {
            return Err(RenetError::ConnectionError(connection_error.clone()));
        }

        loop {
            let payload = match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if addr == *self.connection.addr() {
                        &self.buffer[..len]
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

        self.connection.update();
        Ok(())
    }
}
