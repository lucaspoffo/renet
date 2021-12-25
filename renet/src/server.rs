use crate::channel::reliable::ReliableChannelConfig;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};
use crate::ClientId;

use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug)]
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

pub enum CanConnect {
    Yes,
    No { reason: DisconnectionReason },
}

// TODO: create function to return reference for remote_connection of the given id
// Instead of reemplementing all the functions in connection, we simply make them publicly
#[derive(Debug)]
pub struct Server<C: ClientId> {
    // TODO: what we do with this config
    // We will use only max_players
    config: ServerConfig,
    connections: HashMap<C, RemoteConnection>,
    reliable_channels_config: Vec<ReliableChannelConfig>,
    connection_config: ConnectionConfig,
    disconnected_clients: Vec<(C, DisconnectionReason)>,
}

impl<C: ClientId> Server<C> {
    pub fn new(config: ServerConfig, connection_config: ConnectionConfig, reliable_channels_config: Vec<ReliableChannelConfig>) -> Self {
        Self {
            connections: HashMap::new(),
            config,
            reliable_channels_config,
            connection_config,
            disconnected_clients: Vec::new(),
        }
    }

    pub fn add_connection(&mut self, connection_id: &C) -> Result<(), DisconnectionReason> {
        if let CanConnect::No { reason } = self.can_client_connect(connection_id) {
            return Err(reason);
        }
        let connection = RemoteConnection::new(self.connection_config.clone(), self.reliable_channels_config.clone());
        self.connections.insert(*connection_id, connection);
        Ok(())
    }

    pub fn has_clients(&self) -> bool {
        !self.connections.is_empty()
    }

    pub fn disconnected_client(&mut self) -> Option<(C, DisconnectionReason)> {
        self.disconnected_clients.pop()
    }

    pub fn can_client_connect(&self, connection_id: &C) -> CanConnect {
        if self.connections.contains_key(connection_id) {
            return CanConnect::No {
                reason: DisconnectionReason::ClientAlreadyConnected,
            };
        }

        if self.connections.len() == self.config.max_clients {
            return CanConnect::No {
                reason: DisconnectionReason::MaxConnections,
            };
        }

        CanConnect::Yes
    }

    pub fn network_info(&self, connection_id: C) -> Option<&NetworkInfo> {
        if let Some(connection) = self.connections.get(&connection_id) {
            return Some(connection.network_info());
        }
        None
    }

    pub fn disconnect(&mut self, connection_id: &C) {
        if self.connections.remove(connection_id).is_some() {
            self.disconnected_clients
                .push((*connection_id, DisconnectionReason::DisconnectedByServer));
        }
    }

    pub fn disconnect_all(&mut self) {
        for connection_id in self.connections_id().iter() {
            self.disconnect(connection_id);
        }
    }

    pub fn broadcast_reliable_message(&mut self, channel_id: u8, message: Vec<u8>) {
        for (connection_id, connection) in self.connections.iter_mut() {
            if let Err(e) = connection.send_reliable_message(channel_id, message.clone()) {
                log::error!("Failed to broadcast unreliable message to {:?}: {}", connection_id, e)
            }
        }
    }

    pub fn broadcast_reliable_message_except(&mut self, except_id: &C, channel_id: u8, message: Vec<u8>) {
        for (connection_id, connection) in self.connections.iter_mut() {
            if except_id == connection_id {
                continue;
            }

            if let Err(e) = connection.send_reliable_message(channel_id, message.clone()) {
                log::error!("Failed to broadcast unreliable message to {:?}: {}", connection_id, e)
            }
        }
    }

    pub fn send_reliable_message(&mut self, connection_id: &C, channel_id: u8, message: Vec<u8>) -> Result<(), RenetError> {
        if let Some(remote_connection) = self.connections.get_mut(connection_id) {
            remote_connection.send_reliable_message(channel_id, message)
        } else {
            Err(RenetError::ClientNotFound)
        }
    }

    pub fn broadcast_unreliable_message(&mut self, message: Vec<u8>) {
        for (connection_id, connection) in self.connections.iter_mut() {
            if let Err(e) = connection.send_unreliable_message(message.clone()) {
                log::error!("Failed to broadcast unreliable message to {:?}: {}", connection_id, e)
            }
        }
    }

    pub fn broadcast_unreliable_message_except(&mut self, except_id: &C, message: Vec<u8>) {
        for (connection_id, connection) in self.connections.iter_mut() {
            if except_id == connection_id {
                continue;
            }

            if let Err(e) = connection.send_unreliable_message(message.clone()) {
                log::error!("Failed to broadcast unreliable message to {:?}: {}", connection_id, e)
            }
        }
    }

    pub fn send_unreliable_message(&mut self, connection_id: &C, message: Vec<u8>) -> Result<(), RenetError> {
        if let Some(remote_connection) = self.connections.get_mut(connection_id) {
            remote_connection.send_unreliable_message(message)
        } else {
            Err(RenetError::ClientNotFound)
        }
    }

    pub fn broadcast_block_message(&mut self, message: Vec<u8>) {
        for (connection_id, connection) in self.connections.iter_mut() {
            if let Err(e) = connection.send_block_message(message.clone()) {
                log::error!("Failed to broadcast unreliable message to {:?}: {}", connection_id, e)
            }
        }
    }

    pub fn broadcast_block_message_except(&mut self, except_id: &C, message: Vec<u8>) {
        for (connection_id, connection) in self.connections.iter_mut() {
            if except_id == connection_id {
                continue;
            }

            if let Err(e) = connection.send_block_message(message.clone()) {
                log::error!("Failed to broadcast unreliable message to {:?}: {}", connection_id, e)
            }
        }
    }

    pub fn send_block_message(&mut self, connection_id: &C, message: Vec<u8>) -> Result<(), RenetError> {
        if let Some(remote_connection) = self.connections.get_mut(connection_id) {
            remote_connection.send_block_message(message)
        } else {
            Err(RenetError::ClientNotFound)
        }
    }

    pub fn receive_reliable_message(&mut self, connection_id: &C, channel_id: u8) -> Option<Payload> {
        if let Some(connection) = self.connections.get_mut(connection_id) {
            return connection.receive_reliable_message(channel_id);
        }
        None
    }

    pub fn receive_unreliable_message(&mut self, connection_id: &C) -> Option<Payload> {
        if let Some(connection) = self.connections.get_mut(connection_id) {
            return connection.receive_unreliable_message();
        }
        None
    }

    pub fn receive_block_message(&mut self, connection_id: &C) -> Option<Payload> {
        if let Some(connection) = self.connections.get_mut(connection_id) {
            return connection.receive_block_message();
        }
        None
    }

    pub fn connections_id(&self) -> Vec<C> {
        self.connections.keys().copied().collect()
    }

    pub fn is_client_connected(&self, connection_id: &C) -> bool {
        self.connections.contains_key(connection_id)
    }

    pub fn update_connections(&mut self, duration: Duration) {
        let mut disconnected_clients: Vec<(C, DisconnectionReason)> = vec![];
        for (&connection_id, connection) in self.connections.iter_mut() {
            connection.advance_time(duration);
            if connection.update().is_err() {
                let reason = connection.disconnected().unwrap();
                disconnected_clients.push((connection_id, reason));
            }
        }

        for &(connection_id, reason) in disconnected_clients.iter() {
            self.disconnected_clients.push((connection_id, reason));
        }
    }

    pub fn get_packets_to_send(&mut self, connection_id: &C) -> Result<Vec<Payload>, RenetError> {
        match self.connections.get_mut(connection_id) {
            Some(connection) => connection.get_packets_to_send(),
            None => Err(RenetError::ClientNotFound),
        }
    }

    pub fn process_payload_from(&mut self, payload: &[u8], connection_id: &C) -> Result<(), RenetError> {
        match self.connections.get_mut(connection_id) {
            Some(connection) => connection.process_packet(payload),
            None => Err(RenetError::ClientNotFound),
        }
    }
}
