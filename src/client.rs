use crate::channel::ChannelConfig;
use crate::connection::{ClientId, Connection};
use crate::endpoint::{Endpoint, EndpointConfig, NetworkInfo};
use crate::error::RenetError;
use crate::protocol::{AuthenticationProtocol, SecurityService};
use crate::Timer;

use log::{debug, error, info};

use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;

pub struct RequestConnection {
    socket: UdpSocket,
    server_addr: SocketAddr,
    id: ClientId,
    protocol: Box<dyn AuthenticationProtocol>,
    endpoint_config: EndpointConfig,
    channels_config: HashMap<u8, ChannelConfig>,
    buffer: Box<[u8]>,
    timeout_timer: Timer,
}

impl RequestConnection {
    pub fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        protocol: Box<dyn AuthenticationProtocol>,
        endpoint_config: EndpointConfig,
        channels_config: HashMap<u8, ChannelConfig>,
    ) -> Result<Self, RenetError> {
        socket.set_nonblocking(true)?;
        let buffer = vec![0; endpoint_config.max_packet_size].into_boxed_slice();
        let timeout_timer = Timer::new(endpoint_config.timeout_duration);
        Ok(Self {
            id,
            socket,
            server_addr,
            protocol,
            channels_config,
            endpoint_config,
            timeout_timer,
            buffer,
        })
    }

    fn process_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        self.timeout_timer.reset();
        self.protocol.read_payload(payload)?;
        Ok(())
    }

    pub fn update(&mut self) -> Result<Option<ClientConnected>, RenetError> {
        self.process_events()?;

        if self.timeout_timer.is_finished() {
            error!("Connection with the server timed out");
            return Err(RenetError::ConnectionTimedOut);
        }

        if self.protocol.is_authenticated() {
            let endpoint = Endpoint::new(self.endpoint_config.clone());
            let security_service = self.protocol.build_security_interface();
            return Ok(Some(ClientConnected::new(
                self.id,
                self.socket.try_clone()?,
                self.server_addr,
                endpoint,
                self.channels_config.clone(),
                security_service,
            )));
        }

        match self.protocol.create_payload() {
            Ok(Some(payload)) => {
                info!("Sending protocol payload to server: {:?}", payload);
                self.socket.send_to(&payload, self.server_addr)?;
            }
            Ok(None) => {}
            Err(e) => error!("Failed to create protocol payload: {:?}", e),
        }

        Ok(None)
    }

    fn process_events(&mut self) -> Result<(), RenetError> {
        loop {
            let payload = match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        self.buffer[..len].to_vec().into_boxed_slice()
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(RenetError::IOError(e)),
            };

            self.process_payload(&payload)?;
        }
    }
}

pub struct ClientConnected {
    socket: UdpSocket,
    id: ClientId,
    connection: Connection,
    buffer: Box<[u8]>,
}

impl ClientConnected {
    pub fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        endpoint: Endpoint,
        channels_config: HashMap<u8, ChannelConfig>,
        security_service: Box<dyn SecurityService>,
    ) -> Self {
        let buffer = vec![0; endpoint.config().max_packet_size].into_boxed_slice();
        let mut connection = Connection::new(server_addr, endpoint, security_service);

        for (channel_id, channel_config) in channels_config.iter() {
            let channel = channel_config.new_channel(Instant::now());
            connection.add_channel(*channel_id, channel);
        }

        Self {
            id,
            socket,
            connection,
            buffer,
        }
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

    pub fn process_events(&mut self, current_time: Instant) -> Result<(), RenetError> {
        self.connection.update_channels_current_time(current_time);
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
