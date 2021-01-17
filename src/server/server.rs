use crate::channel::ChannelConfig;
use crate::connection::{ClientId, Connection};
use crate::endpoint::{Endpoint, EndpointConfig, NetworkInfo};
use crate::error::RenetError;
use crate::protocol::{AuthenticationProtocol, ServerAuthenticationProtocol};
use log::{debug, error, info};
use std::collections::HashMap;
use std::io;
use std::marker::PhantomData;
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;

use super::handle_connection::HandleConnection;
use super::ServerConfig;

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected(ClientId),
    ClientDisconnected(ClientId),
}

// TODO: add internal buffer?
pub struct Server<P> {
    config: ServerConfig,
    socket: UdpSocket,
    clients: HashMap<ClientId, Connection>,
    connecting: HashMap<ClientId, HandleConnection>,
    channels_config: HashMap<u8, ChannelConfig>,
    current_time: Instant,
    events: Vec<ServerEvent>,
    endpoint_config: EndpointConfig,
    _authentication_protocol: PhantomData<P>,
}

impl<P> Server<P>
where
    P: AuthenticationProtocol + ServerAuthenticationProtocol,
{
    pub fn new(
        socket: UdpSocket,
        config: ServerConfig,
        endpoint_config: EndpointConfig,
        channels_config: HashMap<u8, ChannelConfig>,
    ) -> Result<Self, RenetError> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            clients: HashMap::new(),
            connecting: HashMap::new(),
            config,
            channels_config,
            endpoint_config,
            current_time: Instant::now(),
            events: Vec::new(),
            _authentication_protocol: PhantomData,
        })
    }

    pub fn has_clients(&self) -> bool {
        !self.clients.is_empty()
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop()
    }

    fn find_client_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut Connection> {
        self.clients.values_mut().find(|c| c.addr == *addr)
    }

    fn find_connection_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut HandleConnection> {
        self.connecting.values_mut().find(|c| c.addr == *addr)
    }

    pub fn get_client_network_info(&mut self, client_id: ClientId) -> Option<&NetworkInfo> {
        if let Some(connection) = self.clients.get_mut(&client_id) {
            connection.endpoint.update_sent_bandwidth();
            connection.endpoint.update_received_bandwidth();
            return Some(connection.endpoint.network_info());
        }
        None
    }

    pub fn send_message_to_all_clients(&mut self, channel_id: u8, message: Box<[u8]>) {
        for connection in self.clients.values_mut() {
            connection.send_message(channel_id, message.clone());
        }
    }

    pub fn send_message_to_client(
        &mut self,
        client_id: ClientId,
        channel_id: u8,
        message: Box<[u8]>,
    ) {
        if let Some(connection) = self.clients.get_mut(&client_id) {
            connection.send_message(channel_id, message);
        }
    }

    pub fn get_messages_from_client(
        &mut self,
        client_id: ClientId,
        channel_id: u8,
    ) -> Option<Vec<Box<[u8]>>> {
        if let Some(client) = self.clients.get_mut(&client_id) {
            return Some(client.receive_all_messages_from_channel(channel_id));
        }
        None
    }

    pub fn get_messages_from_channel(&mut self, channel_id: u8) -> Vec<(ClientId, Vec<Box<[u8]>>)> {
        let mut clients = Vec::new();
        for (client_id, connection) in self.clients.iter_mut() {
            let messages = connection.receive_all_messages_from_channel(channel_id);
            if !messages.is_empty() {
                clients.push((*client_id, messages));
            }
        }
        clients
    }

    pub fn get_clients_id(&self) -> Vec<ClientId> {
        self.clients.keys().map(|x| x.clone()).collect()
    }

    pub fn update(&mut self, current_time: Instant) {
        if let Err(e) = self.process_events(current_time) {
            error!("Error while processing events:\n{:?}", e);
        }
        self.update_pending_connections();
    }

    pub fn send_packets(&mut self) {
        for (client_id, connection) in self.clients.iter_mut() {
            match connection.get_packet() {
                Ok(Some(payload)) => {
                    if let Err(e) = connection.send_payload(&payload, &self.socket) {
                        error!("Failed to send payload for client {}: {:?}", client_id, e);
                    }
                }
                Ok(None) => {}
                Err(_) => error!("Failed to get packet for client {}.", client_id),
            }
        }
    }

    pub fn process_payload_from(
        &mut self,
        payload: &[u8],
        addr: &SocketAddr,
    ) -> Result<(), RenetError> {
        if let Some(client) = self.find_client_by_addr(addr) {
            return client.process_payload(payload);
        }

        if self.clients.len() >= self.config.max_clients {
            // TODO: send denied connection
            debug!("Connection Denied to addr {}, server is full.", addr);
            return Ok(());
        }

        match self.find_connection_by_addr(addr) {
            Some(connection) => {
                if let Err(e) = connection.process_payload(payload) {
                    error!("{}", e)
                }
            }
            None => {
                let protocol = P::from_payload(payload)?;
                let id = protocol.id();
                info!("Created new protocol from payload with client id {}", id);
                let new_connection = HandleConnection::new(
                    protocol.id(),
                    addr.clone(),
                    protocol,
                    self.endpoint_config.timeout_duration,
                );
                self.connecting.insert(id, new_connection);
            }
        };

        Ok(())
    }

    fn process_events(&mut self, current_time: Instant) -> Result<(), RenetError> {
        for connection in self.clients.values_mut() {
            connection.update_channels_current_time(current_time);
        }
        let mut buffer = vec![0u8; self.config.max_payload_size];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if let Err(e) = self.process_payload_from(&buffer[..len], &addr) {
                        error!("Error while processing events:\n{:?}", e);
                    }
                }
                // Break from the loop if would block
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(e) => return Err(RenetError::IOError(e)),
            };
        }
    }

    fn update_pending_connections(&mut self) {
        let mut connected_clients = vec![];
        let mut disconnected_clients = vec![];
        for handle_connection in self.connecting.values_mut() {
            if handle_connection.has_timed_out() {
                disconnected_clients.push(handle_connection.client_id);
                continue;
            }

            if handle_connection.protocol.is_authenticated() {
                connected_clients.push(handle_connection.client_id);
            } else {
                if let Ok(Some(payload)) = handle_connection.protocol.create_payload() {
                    if let Err(e) = self.socket.send_to(&payload, handle_connection.addr) {
                        error!("Failed to send protocol packet {}", e);
                    }
                }
            }
        }

        for client_id in disconnected_clients {
            let handle_connection = self.connecting.remove(&client_id).unwrap();
            self.events
                .push(ServerEvent::ClientDisconnected(handle_connection.client_id));
            info!("Client {} disconnected.", handle_connection.client_id);
        }

        for client_id in connected_clients {
            let handle_connection = self.connecting.remove(&client_id).unwrap();
            if self.clients.len() >= self.config.max_clients {
                info!(
                    "Connection from {} successfuly stablished but server was full.",
                    handle_connection.addr
                );
                // TODO: deny connection, max player
                continue;
            }

            info!(
                "Connection stablished with client {} ({}).",
                handle_connection.client_id, handle_connection.addr,
            );

            let endpoint: Endpoint = Endpoint::new(self.endpoint_config.clone());
            let security_service = handle_connection.protocol.build_security_interface();
            let mut connection =
                Connection::new(handle_connection.addr, endpoint, security_service);

            for (channel_id, channel_config) in self.channels_config.iter() {
                let channel = channel_config.new_channel(self.current_time);
                connection.add_channel(*channel_id, channel);
            }

            self.events
                .push(ServerEvent::ClientConnected(handle_connection.client_id));
            self.clients.insert(handle_connection.client_id, connection);
        }
    }
}
