use crate::channel::reliable::ReliableChannelConfig;
use crate::error::{ClientNotFound, DisconnectionReason, RenetError};
use crate::packet::{Packet, Payload};
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};
use crate::ClientId;

use bincode::Options;
use log::{error, info};

use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionPermission {
    /// All connections are allowed, except clients that are denied.
    All,
    /// Only clients in the allow list can connect.
    OnlyAllowed,
    /// Connections can't be stablished.
    None,
}

/// Determines which clients should receive the message
pub enum SendTarget<C> {
    All,
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
    ClientDisconnected(C, DisconnectionReason),
}

pub enum CanConnect {
    Yes,
    No { reason: DisconnectionReason },
}

pub struct Server<C: ClientId> {
    // TODO: what we do with this config
    // We will use only max_players
    config: ServerConfig,
    clients: HashMap<C, RemoteConnection>,
    reliable_channels_config: Vec<ReliableChannelConfig>,
    events: VecDeque<ServerEvent<C>>,
    connection_config: ConnectionConfig,
    allow_clients: HashSet<C>, // TODO: move again allow/deny logic to another struct
    deny_clients: HashSet<C>,
    connection_permission: ConnectionPermission,
    disconnect_packets: Vec<(C, Payload)>,
}

impl<C: ClientId> Server<C> {
    pub fn new(
        config: ServerConfig,
        connection_config: ConnectionConfig,
        connection_permission: ConnectionPermission,
        reliable_channels_config: Vec<ReliableChannelConfig>,
    ) -> Self {
        Self {
            clients: HashMap::new(),
            config,
            reliable_channels_config,
            connection_config,
            connection_permission,
            allow_clients: HashSet::new(),
            deny_clients: HashSet::new(),
            events: VecDeque::new(),
            disconnect_packets: Vec::new(),
        }
    }

    // TODO: verify if connection is possible
    // TODO: return disconnect packet if cannot connect
    //       or push to self.disconnect_packets
    pub fn add_connection(&mut self, client_id: &C) -> Result<(), DisconnectionReason> {
        if let CanConnect::No { reason } = self.can_client_connect(client_id) {
            return Err(reason);
        }
        self.events
            .push_back(ServerEvent::ClientConnected(*client_id));
        let connection = RemoteConnection::new(
            self.connection_config.clone(),
            self.reliable_channels_config.clone(),
        );
        self.clients.insert(*client_id, connection);
        Ok(())
    }

    pub fn has_clients(&self) -> bool {
        !self.clients.is_empty()
    }

    // TODO: use Drain<ServerEvent<C>>
    pub fn get_event(&mut self) -> Option<ServerEvent<C>> {
        self.events.pop_front()
    }

    pub fn allow_client(&mut self, client_id: &C) {
        self.allow_clients.insert(*client_id);
        self.deny_clients.remove(client_id);
    }

    pub fn allow_connected_clients(&mut self) {
        for client_id in self.get_clients_id().iter() {
            self.allow_client(client_id);
        }
    }

    pub fn deny_client(&mut self, client_id: &C) {
        self.deny_clients.insert(*client_id);
        self.allow_clients.remove(client_id);
    }

    pub fn set_connection_permission(&mut self, connection_permission: ConnectionPermission) {
        self.connection_permission = connection_permission;
    }

    pub fn connection_permission(&self) -> &ConnectionPermission {
        &self.connection_permission
    }

    pub fn can_client_connect(&self, client_id: &C) -> CanConnect {
        if self.deny_clients.contains(client_id) {
            return CanConnect::No {
                reason: DisconnectionReason::Denied,
            };
        }

        if self.clients.contains_key(client_id) {
            return CanConnect::No {
                reason: DisconnectionReason::ClientAlreadyConnected,
            };
        }

        if self.clients.len() == self.config.max_clients {
            return CanConnect::No {
                reason: DisconnectionReason::MaxPlayer,
            };
        }

        match self.connection_permission {
            ConnectionPermission::All => CanConnect::Yes,
            ConnectionPermission::OnlyAllowed => match self.allow_clients.contains(client_id) {
                true => CanConnect::Yes,
                false => CanConnect::No {
                    reason: DisconnectionReason::Denied,
                },
            },

            ConnectionPermission::None => CanConnect::No {
                reason: DisconnectionReason::Denied,
            },
        }
    }

    pub fn allowed_clients(&self) -> Vec<C> {
        self.allow_clients.iter().copied().collect()
    }

    pub fn denied_clients(&self) -> Vec<C> {
        self.deny_clients.iter().copied().collect()
    }

    pub fn network_info(&self, client_id: C) -> Option<&NetworkInfo> {
        if let Some(connection) = self.clients.get(&client_id) {
            return Some(connection.network_info());
        }
        None
    }

    pub fn disconnect(&mut self, client_id: &C) -> Result<(), ClientNotFound> {
        match self.clients.remove(&client_id) {
            Some(_) => {
                self.events.push_back(ServerEvent::ClientDisconnected(
                    *client_id,
                    DisconnectionReason::DisconnectedByServer,
                ));
                Ok(())
            }
            None => Err(ClientNotFound),
        }
    }

    pub fn disconnect_clients(&mut self) -> Vec<C> {
        let client_ids = self.get_clients_id();
        for client_id in client_ids.iter() {
            self.disconnect(client_id).expect("client always exist");
        }
        client_ids
    }

    pub fn send_reliable_message<ChannelId: Into<u8>>(
        &mut self,
        send_target: SendTarget<C>,
        channel_id: ChannelId,
        message: Vec<u8>,
    ) {
        let channel_id = channel_id.into();
        match send_target {
            SendTarget::All => {
                for remote_client in self.clients.values_mut() {
                    remote_client.send_reliable_message(channel_id, message.clone());
                }
            }
            SendTarget::Client(client_id) => {
                if let Some(remote_connection) = self.clients.get_mut(&client_id) {
                    remote_connection.send_reliable_message(channel_id, message);
                } else {
                    error!(
                        "Tried to send reliable message to non-existing client {:?}.",
                        client_id
                    );
                }
            }
        }
    }

    pub fn send_unreliable_message(
        &mut self,
        send_target: SendTarget<C>,
        message: Vec<u8>, // TODO: &[u8] or Bytes
    ) {
        match send_target {
            SendTarget::All => {
                for remote_client in self.clients.values_mut() {
                    remote_client.send_unreliable_message(message.clone());
                }
            }
            SendTarget::Client(client_id) => {
                if let Some(remote_connection) = self.clients.get_mut(&client_id) {
                    remote_connection.send_unreliable_message(message);
                } else {
                    error!(
                        "Tried to send unreliable message to client non-existing client {:?}.",
                        client_id
                    );
                }
            }
        }
    }

    pub fn send_block_message(&mut self, send_target: SendTarget<C>, message: Vec<u8>) {
        match send_target {
            SendTarget::All => {
                for remote_client in self.clients.values_mut() {
                    remote_client.send_block_message(message.clone());
                }
            }
            SendTarget::Client(client_id) => {
                if let Some(remote_connection) = self.clients.get_mut(&client_id) {
                    remote_connection.send_block_message(message);
                } else {
                    error!(
                        "Tried to send block message to client non-existing client {:?}.",
                        client_id
                    );
                }
            }
        }
    }

    pub fn receive_reliable_message(
        &mut self,
        client_id: &C,
        channel_id: u8,
    ) -> Result<Option<Payload>, ClientNotFound> {
        if let Some(remote_client) = self.clients.get_mut(client_id) {
            Ok(remote_client.receive_reliable_message(channel_id))
        } else {
            Err(ClientNotFound)
        }
    }

    pub fn receive_unreliable_message(
        &mut self,
        client_id: &C,
    ) -> Result<Option<Payload>, ClientNotFound> {
        if let Some(remote_client) = self.clients.get_mut(client_id) {
            Ok(remote_client.receive_unreliable_message())
        } else {
            Err(ClientNotFound)
        }
    }

    pub fn receive_block_message(
        &mut self,
        client_id: &C,
    ) -> Result<Option<Payload>, ClientNotFound> {
        if let Some(remote_client) = self.clients.get_mut(client_id) {
            Ok(remote_client.receive_block_message())
        } else {
            Err(ClientNotFound)
        }
    }

    pub fn get_clients_id(&self) -> Vec<C> {
        self.clients.keys().copied().collect()
    }

    pub fn is_client_connected(&self, client_id: &C) -> bool {
        self.clients.contains_key(client_id)
    }

    // TODO: should we return disconnect packets here instead of pushing to a vec?
    pub fn update_connections(&mut self) -> Vec<(C, DisconnectionReason)> {
        let mut disconnected_clients: Vec<(C, DisconnectionReason)> = vec![];
        for (&client_id, connection) in self.clients.iter_mut() {
            if connection.update().is_err() {
                let reason = connection.disconnected().unwrap();
                disconnected_clients.push((client_id, reason));
            }
        }

        for &(client_id, reason) in disconnected_clients.iter() {
            self.clients.remove(&client_id);
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id, reason));
            info!("Client {:?} disconnected.", client_id);
        }
        disconnected_clients
    }

    pub fn get_packets_to_send(&mut self) -> Result<Vec<(C, Payload)>, RenetError> {
        let mut packets: Vec<(C, Payload)> = std::mem::take(&mut self.disconnect_packets);

        for (client_id, connection) in self.clients.iter_mut() {
            for packet in connection.get_packets_to_send()?.into_iter() {
                packets.push((*client_id, packet));
            }
        }

        Ok(packets)
    }

    pub fn process_payload_from(&mut self, payload: &[u8], client_id: C) -> Result<(), RenetError> {
        if let Some(connection) = self.clients.get_mut(&client_id) {
            connection.process_packet(payload)?;
        }

        Ok(())
    }
}
