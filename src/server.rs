use crate::channel::reliable::ReliableChannelConfig;
use crate::client::{LocalClient, LocalClientConnected};
use crate::error::{ClientNotFound, DisconnectionReason, RenetError};
use crate::packet::Payload;
use crate::protocol::ServerAuthenticationProtocol;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};
use crate::ClientId;

use log::{debug, error, info};

use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{SocketAddr, UdpSocket};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionPermission {
    /// All connection are allowed, except clients that are denied.
    All,
    /// Only clients in the allow list can connect.
    OnlyAllowed,
    /// No connection can be stablished.
    None,
}

/// Determines which clients should receive the message
pub enum SendTarget<C> {
    All,
    OnlyRemote,
    Client(C),
    // TODO:
    // AllExcept(C),
}

impl Default for ConnectionPermission {
    fn default() -> Self {
        ConnectionPermission::All
    }
}

pub struct ServerConfig {
    pub max_clients: usize,
    pub max_payload_size: usize,
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
pub enum ServerEvent<C> {
    ClientConnected(C),
    ClientDisconnected(C),
}

pub struct Server<Protocol: ServerAuthenticationProtocol> {
    config: ServerConfig,
    socket: UdpSocket,
    remote_clients: HashMap<Protocol::ClientId, RemoteConnection<Protocol>>,
    local_clients: HashMap<Protocol::ClientId, LocalClient<Protocol::ClientId>>,
    connecting: HashMap<Protocol::ClientId, RemoteConnection<Protocol>>,
    reliable_channels_config: Vec<ReliableChannelConfig>,
    events: VecDeque<ServerEvent<Protocol::ClientId>>,
    connection_config: ConnectionConfig,
    allow_clients: HashSet<Protocol::ClientId>,
    deny_clients: HashSet<Protocol::ClientId>,
    connection_permission: ConnectionPermission,
}

impl<Protocol> Server<Protocol>
where
    Protocol: ServerAuthenticationProtocol,
    Protocol::ClientId: ClientId,
{
    pub fn new(
        socket: UdpSocket,
        config: ServerConfig,
        connection_config: ConnectionConfig,
        connection_permission: ConnectionPermission,
        reliable_channels_config: Vec<ReliableChannelConfig>,
    ) -> Result<Self, io::Error> {
        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            remote_clients: HashMap::new(),
            local_clients: HashMap::new(),
            connecting: HashMap::new(),
            config,
            reliable_channels_config,
            connection_config,
            connection_permission,
            allow_clients: HashSet::new(),
            deny_clients: HashSet::new(),
            events: VecDeque::new(),
        })
    }

    pub fn addr(&self) -> Option<SocketAddr> {
        self.socket.local_addr().ok()
    }

    pub fn has_clients(&self) -> bool {
        !self.remote_clients.is_empty() || !self.local_clients.is_empty()
    }

    pub fn get_event(&mut self) -> Option<ServerEvent<Protocol::ClientId>> {
        self.events.pop_front()
    }

    fn find_client_by_addr(
        &mut self,
        addr: &SocketAddr,
    ) -> Option<&mut RemoteConnection<Protocol>> {
        self.remote_clients
            .values_mut()
            .find(|c| *c.addr() == *addr)
    }

    fn find_connecting_by_addr(
        &mut self,
        addr: &SocketAddr,
    ) -> Option<&mut RemoteConnection<Protocol>> {
        self.connecting.values_mut().find(|c| c.addr() == addr)
    }

    pub fn allow_client(&mut self, client_id: &Protocol::ClientId) {
        self.allow_clients.insert(*client_id);
        self.deny_clients.remove(client_id);
    }

    pub fn allow_connected_clients(&mut self) {
        for client_id in self.get_clients_id().iter() {
            self.allow_client(client_id);
        }
    }

    pub fn deny_client(&mut self, client_id: &Protocol::ClientId) {
        self.deny_clients.insert(*client_id);
        self.allow_clients.remove(client_id);
    }

    pub fn set_connection_permission(&mut self, connection_permission: ConnectionPermission) {
        self.connection_permission = connection_permission;
    }

    pub fn connection_permission(&self) -> &ConnectionPermission {
        &self.connection_permission
    }

    pub fn can_client_connect(&self, client_id: Protocol::ClientId) -> bool {
        if self.deny_clients.contains(&client_id) {
            return false;
        }

        match self.connection_permission {
            ConnectionPermission::All => true,
            ConnectionPermission::OnlyAllowed => self.allow_clients.contains(&client_id),
            ConnectionPermission::None => false,
        }
    }

    pub fn allowed_clients(&self) -> Vec<Protocol::ClientId> {
        self.allow_clients.iter().copied().collect()
    }

    pub fn denied_clients(&self) -> Vec<Protocol::ClientId> {
        self.deny_clients.iter().copied().collect()
    }

    pub fn network_info(&self, client_id: Protocol::ClientId) -> Option<&NetworkInfo> {
        if let Some(connection) = self.remote_clients.get(&client_id) {
            return Some(connection.network_info());
        }
        None
    }

    pub fn create_local_client(
        &mut self,
        client_id: Protocol::ClientId,
    ) -> LocalClientConnected<Protocol::ClientId> {
        self.events
            .push_back(ServerEvent::ClientConnected(client_id));
        let (local_client_connected, local_client) = LocalClientConnected::new(client_id);
        self.local_clients.insert(client_id, local_client);
        local_client_connected
    }

    pub fn disconnect(&mut self, client_id: &Protocol::ClientId) {
        if let Some(mut remote_client) = self.remote_clients.remove(&client_id) {
            self.events
                .push_back(ServerEvent::ClientDisconnected(*client_id));
            if let Err(e) = remote_client
                .send_disconnect_packet(&self.socket, DisconnectionReason::DisconnectedByServer)
            {
                error!(
                    "Failed to send disconnect packet to Client {:?}: {}",
                    client_id, e
                );
            }
        } else if let Some(mut local_client) = self.local_clients.remove(&client_id) {
            local_client.disconnect();
            self.events
                .push_back(ServerEvent::ClientDisconnected(*client_id));
        }
    }

    pub fn disconnect_clients(&mut self) {
        for client_id in self.get_clients_id().iter() {
            self.disconnect(client_id);
        }
    }

    pub fn send_reliable_message<C: Into<u8>>(
        &mut self,
        send_target: SendTarget<Protocol::ClientId>,
        channel_id: C,
        message: Vec<u8>,
    ) {
        let channel_id = channel_id.into();
        match send_target {
            SendTarget::All => {
                for remote_client in self.remote_clients.values_mut() {
                    remote_client.send_reliable_message(channel_id, message.clone());
                }

                for local_client in self.local_clients.values_mut() {
                    local_client.send_message(message.clone());
                }
            }
            SendTarget::OnlyRemote => {
                for remote_client in self.remote_clients.values_mut() {
                    remote_client.send_reliable_message(channel_id, message.clone());
                }
            }
            SendTarget::Client(client_id) => {
                if let Some(remote_connection) = self.remote_clients.get_mut(&client_id) {
                    remote_connection.send_reliable_message(channel_id, message);
                } else if let Some(local_connection) = self.local_clients.get_mut(&client_id) {
                    local_connection.send_message(message);
                } else {
                    error!(
                        "Tried to send reliable message to client non-existing client {:?}.",
                        client_id
                    );
                }
            }
        }
    }

    pub fn send_unreliable_message(
        &mut self,
        send_target: SendTarget<Protocol::ClientId>,
        message: Vec<u8>,
    ) {
        match send_target {
            SendTarget::All => {
                for remote_client in self.remote_clients.values_mut() {
                    remote_client.send_unreliable_message(message.clone());
                }

                for local_client in self.local_clients.values_mut() {
                    local_client.send_message(message.clone());
                }
            }
            SendTarget::OnlyRemote => {
                for remote_client in self.remote_clients.values_mut() {
                    remote_client.send_unreliable_message(message.clone());
                }
            }
            SendTarget::Client(client_id) => {
                if let Some(remote_connection) = self.remote_clients.get_mut(&client_id) {
                    remote_connection.send_unreliable_message(message);
                } else if let Some(local_connection) = self.local_clients.get_mut(&client_id) {
                    local_connection.send_message(message);
                } else {
                    error!(
                        "Tried to send unreliable message to client non-existing client {:?}.",
                        client_id
                    );
                }
            }
        }
    }

    pub fn send_block_message(
        &mut self,
        send_target: SendTarget<Protocol::ClientId>,
        message: Vec<u8>,
    ) {
        match send_target {
            SendTarget::All => {
                for remote_client in self.remote_clients.values_mut() {
                    remote_client.send_block_message(message.clone());
                }

                for local_client in self.local_clients.values_mut() {
                    local_client.send_message(message.clone());
                }
            }
            SendTarget::OnlyRemote => {
                for remote_client in self.remote_clients.values_mut() {
                    remote_client.send_block_message(message.clone());
                }
            }
            SendTarget::Client(client_id) => {
                if let Some(remote_connection) = self.remote_clients.get_mut(&client_id) {
                    remote_connection.send_block_message(message);
                } else if let Some(local_connection) = self.local_clients.get_mut(&client_id) {
                    local_connection.send_message(message);
                } else {
                    error!(
                        "Tried to send block message to client non-existing client {:?}.",
                        client_id
                    );
                }
            }
        }
    }

    pub fn receive_message(
        &mut self,
        client_id: Protocol::ClientId,
    ) -> Result<Option<Payload>, ClientNotFound> {
        if let Some(remote_client) = self.remote_clients.get_mut(&client_id) {
            Ok(remote_client.receive_message())
        } else if let Some(local_client) = self.local_clients.get_mut(&client_id) {
            Ok(local_client.receive_message())
        } else {
            Err(ClientNotFound)
        }
    }

    pub fn get_clients_id(&self) -> Vec<Protocol::ClientId> {
        self.remote_clients
            .keys()
            .copied()
            .chain(self.local_clients.keys().copied())
            .collect()
    }

    pub fn is_client_connected(&self, client_id: &Protocol::ClientId) -> bool {
        self.remote_clients.contains_key(client_id) || self.local_clients.contains_key(client_id)
    }

    pub fn update(&mut self) -> Result<(), io::Error> {
        let mut buffer = vec![0u8; self.connection_config.max_packet_size as usize];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => match self.process_payload_from(&buffer[..len], &addr) {
                    Err(RenetError::IOError(e)) => return Err(e),
                    Err(e) => error!("{}", e),
                    Ok(()) => {}
                },
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            };
        }

        let mut disconnected_remote_clients: Vec<Protocol::ClientId> = vec![];
        for (&client_id, connection) in self.remote_clients.iter_mut() {
            if connection.update().is_err() {
                disconnected_remote_clients.push(client_id);
            }
        }

        for &client_id in disconnected_remote_clients.iter() {
            self.remote_clients.remove(&client_id);
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id));
            info!("Remote Client {:?} disconnected.", client_id);
        }

        let mut disconnected_local_clients: Vec<Protocol::ClientId> = vec![];
        for (&client_id, connection) in self.local_clients.iter() {
            if !connection.is_connected() {
                disconnected_local_clients.push(client_id);
            }
        }

        for &client_id in disconnected_local_clients.iter() {
            self.local_clients.remove(&client_id);
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id));
            info!("Local Client {:?} disconnected.", client_id);
        }

        match self.update_pending_connections() {
            Err(RenetError::IOError(e)) => Err(e),
            Err(e) => {
                error!("Error updating pending connections: {}", e);
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    fn update_pending_connections(&mut self) -> Result<(), RenetError> {
        let mut connected_clients = vec![];
        let mut disconnected_clients = vec![];
        for connection in self.connecting.values_mut() {
            if connection.update().is_err() {
                disconnected_clients.push(connection.client_id());
                continue;
            }

            if connection.is_connected() {
                connected_clients.push(connection.client_id());
            } else if let Ok(Some(payload)) = connection.create_protocol_payload() {
                try_send_packet(&self.socket, payload, connection.addr())?;
            }
        }

        for client_id in disconnected_clients {
            self.connecting
                .remove(&client_id)
                .expect("Disconnected Clients always exists");
            info!("Request connection {:?} failed.", client_id);
        }

        for client_id in connected_clients {
            let mut connection = self
                .connecting
                .remove(&client_id)
                .expect("Connected Client always exist");
            if self.remote_clients.len() >= self.config.max_clients {
                info!(
                    "Connection from {} successfuly stablished but server was full.",
                    connection.addr()
                );
                connection.try_send_disconnect_packet(&self.socket, DisconnectionReason::MaxPlayer);

                continue;
            }

            info!(
                "Connection stablished with client {:?} ({}).",
                connection.client_id(),
                connection.addr(),
            );

            self.events
                .push_back(ServerEvent::ClientConnected(connection.client_id()));
            self.remote_clients
                .insert(connection.client_id(), connection);
        }

        Ok(())
    }

    pub fn send_packets(&mut self) {
        for (client_id, connection) in self.remote_clients.iter_mut() {
            if let Err(e) = connection.send_packets(&self.socket) {
                error!("Failed to send packet for Client {:?}: {:?}", client_id, e);
            }
        }
    }

    fn process_payload_from(
        &mut self,
        payload: &[u8],
        addr: &SocketAddr,
    ) -> Result<(), RenetError> {
        if let Some(client) = self.find_client_by_addr(addr) {
            return client.process_payload(payload);
        }

        if self.remote_clients.len() >= self.config.max_clients {
            let packet = Protocol::get_disconnect_packet(DisconnectionReason::MaxPlayer);
            try_send_packet(&self.socket, packet, addr)?;
            debug!("Connection Denied to addr {}, server is full.", addr);
            return Ok(());
        }

        match self.find_connecting_by_addr(addr) {
            Some(connection) => {
                return connection.process_payload(payload);
            }
            None => {
                let protocol =
                    Protocol::from_payload(&payload).map_err(RenetError::AuthenticationError)?;
                let id = protocol.id();
                if !self.can_client_connect(id) {
                    info!(
                        "Client with id {:?} is not allowed to connect the server.",
                        id
                    );

                    let packet = Protocol::get_disconnect_packet(DisconnectionReason::Denied);
                    try_send_packet(&self.socket, packet, addr)?;

                    return Ok(());
                }

                if self.is_client_connected(&id) {
                    info!(
                        "Client with id {:?} already connected, discarded connection attempt.",
                        id
                    );

                    let packet = Protocol::get_disconnect_packet(
                        DisconnectionReason::ClientIdAlreadyConnected,
                    );
                    try_send_packet(&self.socket, packet, addr)?;
                } else {
                    info!("Created new protocol from payload with client id {:?}", id);
                    let new_connection = RemoteConnection::new(
                        protocol.id(),
                        *addr,
                        self.connection_config.clone(),
                        protocol,
                        self.reliable_channels_config.clone(),
                    );
                    self.connecting.insert(id, new_connection);
                }
            }
        };

        Ok(())
    }
}

fn try_send_packet(
    socket: &UdpSocket,
    packet: Payload,
    addr: &SocketAddr,
) -> Result<(), io::Error> {
    socket.send_to(&packet, addr)?;
    Ok(())
}
