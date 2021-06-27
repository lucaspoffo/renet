use crate::client::Client;
use crate::connection::{ClientId, Connection, NetworkInfo};
use crate::error::RenetError;
use crate::protocol::SecurityService;
use crate::{channel::ChannelConfig, connection::ConnectionConfig};

use log::debug;

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::collections::HashMap;

pub struct ClientConnected<S> {
    socket: UdpSocket,
    id: ClientId,
    connection: Connection<S>,
    buffer: Box<[u8]>,
}

impl<S: SecurityService> ClientConnected<S> {
    pub(crate) fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
        security_service: S,
        connection_config: ConnectionConfig,
    ) -> Self {
        let buffer = vec![0; connection_config.max_packet_size].into_boxed_slice();
        let mut connection = Connection::new(server_addr, connection_config, security_service);

        for (channel_id, channel_config) in channels_config.iter() {
            let channel = channel_config.new_channel();
            connection.add_channel(*channel_id, channel);
        }

        Self {
            id,
            socket,
            connection,
            buffer,
        }
    }
}

impl<S: SecurityService> Client for ClientConnected<S> {
    fn id(&self) -> ClientId {
        self.id
    }

    fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) {
        self.connection.send_message(channel_id.into(), message);
    }

    fn receive_message(&mut self, channel_id: u8) -> Option<Box<[u8]>> {
        self.connection.receive_message(channel_id.into())
    }

    fn network_info(&mut self) -> &NetworkInfo {
        self.connection.network_info()
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        self.connection.send_packets(&self.socket)?;
        Ok(())
    }

    fn process_events(&mut self) -> Result<(), RenetError> {
        if self.connection.has_timed_out() {
            return Err(RenetError::ConnectionTimedOut);
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
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(RenetError::IOError(e)),
            };

            // TODO: correctly handle error
            self.connection.process_payload(payload)?;
        }
    }
}
