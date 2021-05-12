use crate::channel::ChannelConfig;
use crate::client::{HostClient, HostServer};
use crate::connection::{ClientId, Connection, ConnectionConfig, NetworkInfo};
use crate::error::RenetError;
use crate::protocol::ServerAuthenticationProtocol;

use log::{debug, error, info};

use std::collections::HashMap;
use std::collections::VecDeque;
use std::io;
use std::marker::PhantomData;
use std::net::{SocketAddr, UdpSocket};

use super::handle_connection::HandleConnection;
use super::ServerConfig;

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected(ClientId),
    ClientDisconnected(ClientId),
}

// TODO: add internal buffer?
pub struct Server<P: ServerAuthenticationProtocol, C> {
    config: ServerConfig,
    socket: UdpSocket,
    clients: HashMap<ClientId, Connection<P::Service>>,
    connecting: HashMap<ClientId, HandleConnection<P>>,
    channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
    events: VecDeque<ServerEvent>,
    connection_config: ConnectionConfig,
    host_server: Option<HostServer>,
    _server_authentication_protocol: PhantomData<P>,
    _channel_identifier: PhantomData<C>,
}

impl<P, C: Into<u8>> Server<P, C>
where
    P: ServerAuthenticationProtocol,
{
    pub fn new(
        socket: UdpSocket,
        config: ServerConfig,
        connection_config: ConnectionConfig,
        channels_config: HashMap<C, Box<dyn ChannelConfig>>,
    ) -> Result<Self, RenetError> {
        socket.set_nonblocking(true)?;
        let channels_config: HashMap<u8, Box<dyn ChannelConfig>> = channels_config
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();

        Ok(Self {
            socket,
            clients: HashMap::new(),
            connecting: HashMap::new(),
            config,
            channels_config,
            connection_config,
            events: VecDeque::new(),
            host_server: None,
            _server_authentication_protocol: PhantomData,
            _channel_identifier: PhantomData,
        })
    }

    // TODO: Remove host_client and create an trait for the Client Connection,
    // Impl trait for RemoteConnection and LocalConnection. Save connected clients in same list.
    pub fn create_host_client(&mut self, client_id: u64) -> HostClient<C> {
        let channels = self.channels_config.keys().copied().collect();
        self.events
            .push_back(ServerEvent::ClientConnected(client_id));
        let (host_client, host_server) = HostClient::new(client_id, channels);
        self.host_server = Some(host_server);
        host_client
    }

    pub fn has_clients(&self) -> bool {
        !self.clients.is_empty() || self.host_server.is_some()
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    fn find_client_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut Connection<P::Service>> {
        self.clients.values_mut().find(|c| *c.addr() == *addr)
    }

    fn find_connecting_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut HandleConnection<P>> {
        self.connecting.values_mut().find(|c| c.addr == *addr)
    }

    pub fn get_client_network_info(&mut self, client_id: ClientId) -> Option<&NetworkInfo> {
        if let Some(connection) = self.clients.get_mut(&client_id) {
            return Some(connection.network_info());
        }
        None
    }

    pub fn send_message_to_all_clients(&mut self, channel_id: C, message: Box<[u8]>) {
        let channel_id = channel_id.into();
        for connection in self.clients.values_mut() {
            connection.send_message(channel_id, message.clone());
        }

        if let Some(host) = self.host_server.as_ref() {
            host.send_message(channel_id, message);
        }
    }

    pub fn send_message_to_client(
        &mut self,
        client_id: ClientId,
        channel_id: C,
        message: Box<[u8]>,
    ) {
        let channel_id = channel_id.into();
        if let Some(connection) = self.clients.get_mut(&client_id) {
            connection.send_message(channel_id, message);
        } else if let Some(host) = self.host_server.as_ref() {
            if host.id == client_id {
                host.send_message(channel_id, message);
            }
        }
    }

    pub fn get_messages_from_client(
        &mut self,
        client_id: ClientId,
        channel_id: C,
    ) -> Option<Vec<Box<[u8]>>> {
        let channel_id = channel_id.into();
        if let Some(client) = self.clients.get_mut(&client_id) {
            return Some(client.receive_all_messages_from_channel(channel_id));
        } else if let Some(host) = self.host_server.as_ref() {
            if host.id == client_id {
                return host.receive_messages(channel_id);
            }
        }
        None
    }

    pub fn get_messages_from_channel(&mut self, channel_id: C) -> Vec<(ClientId, Vec<Box<[u8]>>)> {
        let channel_id = channel_id.into();
        let mut clients = Vec::new();
        for (client_id, connection) in self.clients.iter_mut() {
            let messages = connection.receive_all_messages_from_channel(channel_id);
            if !messages.is_empty() {
                clients.push((*client_id, messages));
            }
        }
        if let Some(host) = self.host_server.as_ref() {
            if let Some(messages) = host.receive_messages(channel_id) {
                clients.push((host.id, messages));
            }
        }
        clients
    }

    pub fn get_clients_id(&self) -> Vec<ClientId> {
        let mut clients: Vec<ClientId> = self.clients.keys().copied().collect();
        if let Some(host) = self.host_server.as_ref() {
            clients.push(host.id);
        }
        clients
    }

    pub fn update(&mut self) {
        let mut timed_out_connections: Vec<ClientId> = vec![];
        for (&client_id, connection) in self.clients.iter_mut() {
            if connection.has_timed_out() {
                timed_out_connections.push(client_id);
            }
        }

        for &client_id in timed_out_connections.iter() {
            self.clients.remove(&client_id).unwrap();
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id));
            info!("Client {} disconnected.", client_id);
        }

        if let Err(e) = self.process_events() {
            error!("Error while processing events:\n{:?}", e);
        }
        self.update_pending_connections();
    }

    pub fn send_packets(&mut self) {
        for (client_id, connection) in self.clients.iter_mut() {
            if let Err(e) = connection.send_packets(&self.socket) {
                error!("Failed to send packet for client {}: {:?}", client_id, e);
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

        match self.find_connecting_by_addr(addr) {
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
                    *addr,
                    protocol,
                    self.connection_config.timeout_duration,
                );
                self.connecting.insert(id, new_connection);
            }
        };

        Ok(())
    }

    fn process_events(&mut self) -> Result<(), RenetError> {
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
            } else if let Ok(Some(payload)) = handle_connection.protocol.create_payload() {
                if let Err(e) = self.socket.send_to(&payload, handle_connection.addr) {
                    error!("Failed to send protocol packet {}", e);
                }
            }
        }

        for client_id in disconnected_clients {
            let handle_connection = self.connecting.remove(&client_id).unwrap();
            info!(
                "Request connection {} disconnected.",
                handle_connection.client_id
            );
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

            let security_service = handle_connection.protocol.build_security_interface();
            let mut connection = Connection::new(
                handle_connection.addr,
                self.connection_config.clone(),
                security_service,
            );

            for (channel_id, channel_config) in self.channels_config.iter() {
                let channel = channel_config.new_channel();
                connection.add_channel(*channel_id, channel);
            }

            self.events
                .push_back(ServerEvent::ClientConnected(handle_connection.client_id));
            self.clients.insert(handle_connection.client_id, connection);
        }
    }
}
