use crate::channel::ChannelConfig;
use crate::connection::{ClientId, Connection, HandleConnection};
use crate::endpoint::{Config, Endpoint, NetworkInfo};
use crate::error::RenetError;
use crate::protocol::{AuthenticationProtocol, SecurityService, ServerAuthenticationProtocol};
use log::{debug, error};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;
use std::marker::PhantomData;

// TODO investigate if we can separate the client state
// in the server from the from the client itself
#[derive(Debug, Eq, PartialEq)]
pub enum ClientState {
    Connected,
    Disconnected,
    ConnectionTimedOut,
}

struct Client {
    id: ClientId,
    state: ClientState,
    connection: Connection,
}

impl Client {
    fn new(id: ClientId, addr: SocketAddr, endpoint: Endpoint, security_service: Box<dyn SecurityService>) -> Self {
        Self {
            id,
            state: ClientState::Connected,
            connection: Connection::new(addr, endpoint, security_service),
        }
    }

    fn is_connected(&self) -> bool {
        self.state == ClientState::Connected
    }

    fn receive_all_messages_from_channel(&mut self, channel_id: u8) -> Vec<Box<[u8]>> {
        self.connection
            .receive_all_messages_from_channel(channel_id)
    }

    fn process_payload(&mut self, payload: Box<[u8]>) {
        self.connection.process_payload(payload);
    }
}

pub struct ServerConfig {
    max_clients: usize,
    max_payload_size: usize,
}

impl ServerConfig {
    pub fn new(max_clients: usize, max_payload_size: usize) -> Self {
        Self {
            max_clients,
            max_payload_size,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_clients: 16,
            max_payload_size: 8 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    ClientConnected(ClientId),
    ClientDisconnected(ClientId),
}

// TODO: add internal buffer?
pub struct Server<P> {
    config: ServerConfig,
    socket: UdpSocket,
    clients: HashMap<ClientId, Client>,
    connecting: HashMap<ClientId, HandleConnection>,
    channels_config: HashMap<u8, ChannelConfig>,
    current_time: Instant,
    events: Vec<Event>,
    _authentication_protocol: PhantomData<P>
}

impl<P> Server<P>
where P: AuthenticationProtocol + ServerAuthenticationProtocol,
{
    pub fn new(socket: UdpSocket, config: ServerConfig) -> Result<Self, RenetError> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            clients: HashMap::new(),
            connecting: HashMap::new(),
            config,
            channels_config: HashMap::new(),
            current_time: Instant::now(),
            events: Vec::new(),
            _authentication_protocol: PhantomData
        })
    }

    pub fn add_channel_config(&mut self, channel_id: u8, config: ChannelConfig) {
        self.channels_config.insert(channel_id, config);
    }

    pub fn has_clients(&self) -> bool {
        !self.clients.is_empty()
    }

    pub fn get_events(&self) -> Vec<Event> {
        self.events.clone()
    }

    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    fn find_client_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut Client> {
        self.clients
            .values_mut()
            .find(|c| c.connection.addr() == addr)
    }

    fn find_connection_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut HandleConnection> {
        self.connecting.values_mut().find(|c| c.addr() == addr)
    }

    pub fn get_client_network_info(&mut self, client_id: ClientId) -> Option<&NetworkInfo> {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.connection.endpoint.update_sent_bandwidth();
            client.connection.endpoint.update_received_bandwidth();
            return Some(client.connection.endpoint.network_info());
        }
        None
    }

    pub fn send_message_to_all_clients(&mut self, channel_id: u8, message: Box<[u8]>) {
        for client in self.clients.values_mut() {
            client.connection.send_message(channel_id, message.clone());
        }
    }

    pub fn send_message_to_client(
        &mut self,
        client_id: ClientId,
        channel_id: u8,
        message: Box<[u8]>,
    ) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.connection.send_message(channel_id, message);
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
        for client in self.clients.values_mut() {
            match client.connection.get_packet() {
                Ok(Some(payload)) => {
                    if let Err(e) = client.connection.send_payload(&payload, &self.socket) {
                        error!("Failed to send payload for client {}: {:?}", client.id, e);
                    }
                }
                Ok(None) => {}
                Err(_) => error!("Failed to get packet for client {}.", client.id),
            }
        }
    }

    pub fn process_payload_from(
        &mut self,
        payload: Box<[u8]>,
        addr: &SocketAddr,
    ) -> Result<(), RenetError> {
        if let Some(client) = self.find_client_by_addr(addr) {
            client.process_payload(payload);
            return Ok(());
        }

        if self.clients.len() >= self.config.max_clients {
            // TODO: send denied connection
            debug!("Connection Denied to addr {}, server is full.", addr);
            return Ok(());
        }

        match self.find_connection_by_addr(addr) {
            Some(connection) => {
                connection.process_payload(payload);
            },
            None => {
                let protocol = P::from_payload(payload)?;
                let id = protocol.id();
                let new_connection = HandleConnection::new(protocol.id(), addr.clone(), protocol);
                self.connecting.insert(id, new_connection);
            }
        };

        Ok(())
    }

    fn process_events(&mut self, current_time: Instant) -> Result<(), RenetError> {
        for client in self.clients.values_mut() {
            client.connection.update_channels_current_time(current_time);
        }
        let mut buffer = vec![0u8; self.config.max_payload_size];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    let payload = buffer[..len].to_vec().into_boxed_slice();
                    if let Err(e) = self.process_payload_from(payload, &addr) {
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
        let mut connected_connections = vec![];
        for connection in self.connecting.values() {
                if connection.is_authenticated() {
                    connected_connections.push(connection.client_id());
                }
        }
        for connected in connected_connections {
            let connection = self
                .connecting
                .remove(&connected)
                .expect("Should only connect existing clients.");
            if self.clients.len() >= self.config.max_clients {
                debug!(
                    "Connection from {} successfuly stablished but server was full.",
                    connection.addr()
                );
                // TODO: deny connection, max player
                continue;
            }

            debug!(
                "Connection stablished with client {} ({}).",
                connection.client_id(),
                connection.addr(),
            );
            let endpoint_config = Config::default();
            let endpoint: Endpoint = Endpoint::new(endpoint_config);
            let security_service = connection.protocol.build_security_interface();
            let mut client = Client::new(connection.client_id(), *connection.addr(), endpoint, security_service);
            for (channel_id, channel_config) in self.channels_config.iter() {
                let channel = channel_config.new_channel(self.current_time);
                client.connection.add_channel(*channel_id, channel);
            }
            self.events.push(Event::ClientConnected(client.id));
            self.clients.insert(client.id, client);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, UdpSocket};

    #[test]
    fn server_client_connecting_flow() {
        let socket = UdpSocket::bind("127.0.0.1:8080").unwrap();
        let server_config = ServerConfig::default();
        let mut server = Server::new(socket, server_config).unwrap();

        let client_addr: SocketAddr = "127.0.0.1:8081".parse().unwrap();
        let packet = ConnectionPacket::ConnectionRequest(0);
        server.process_packet_from(packet, &client_addr).unwrap();
        assert_eq!(server.connecting.len(), 1);

        let packet = ConnectionPacket::ChallengeResponse;
        server.process_packet_from(packet, &client_addr).unwrap();
        server.update_pending_connections();
        assert_eq!(server.connecting.len(), 1);
        assert_eq!(server.clients.len(), 0);

        let packet = ConnectionPacket::HeartBeat;
        server.process_packet_from(packet, &client_addr).unwrap();
        server.update_pending_connections();
        assert_eq!(server.connecting.len(), 0);
        assert_eq!(server.clients.len(), 1);
        assert!(server.clients.contains_key(&0));
    }
}
