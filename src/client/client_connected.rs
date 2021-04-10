use crate::channel::ChannelConfig;
use crate::client::Client;
use crate::connection::{ClientId, Connection};
use crate::endpoint::{Endpoint, NetworkInfo};
use crate::error::RenetError;
use crate::protocol::SecurityService;

use log::debug;

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::{collections::HashMap, marker::PhantomData};

pub struct ClientConnected<S, C> {
    socket: UdpSocket,
    id: ClientId,
    connection: Connection<S>,
    buffer: Box<[u8]>,
    _channel: PhantomData<C>,
}

impl<S: SecurityService, C: Into<u8>> ClientConnected<S, C> {
    pub(crate) fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        endpoint: Endpoint,
        channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
        security_service: S,
    ) -> Self {
        let buffer = vec![0; endpoint.config().max_packet_size].into_boxed_slice();
        let mut connection = Connection::new(server_addr, endpoint, security_service);

        for (channel_id, channel_config) in channels_config.iter() {
            let channel = channel_config.new_channel();
            connection.add_channel(*channel_id, channel);
        }

        Self {
            id,
            socket,
            connection,
            buffer,
            _channel: PhantomData,
        }
    }
}

impl<S: SecurityService, C: Into<u8>> Client<C> for ClientConnected<S, C> {
    fn id(&self) -> ClientId {
        self.id
    }

    fn send_message(&mut self, channel_id: C, message: Box<[u8]>) {
        self.connection.send_message(channel_id.into(), message);
    }

    fn receive_all_messages_from_channel(&mut self, channel_id: C) -> Vec<Box<[u8]>> {
        self.connection
            .receive_all_messages_from_channel(channel_id.into())
    }

    fn network_info(&mut self) -> &NetworkInfo {
        self.connection.endpoint.update_sent_bandwidth();
        self.connection.endpoint.update_received_bandwidth();
        self.connection.endpoint.network_info()
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
                    if addr == self.connection.addr {
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
