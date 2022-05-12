use crate::error::{DisconnectionReason, RechannelError};
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};
use crate::ClientId;

use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug)]
pub struct RechannelServer<C: ClientId> {
    connections: HashMap<C, RemoteConnection>,
    connection_config: ConnectionConfig,
    disconnections: Vec<(C, DisconnectionReason)>,
}

impl<C: ClientId> RechannelServer<C> {
    pub fn new(connection_config: ConnectionConfig) -> Self {
        Self {
            connections: HashMap::new(),
            connection_config,
            disconnections: Vec::new(),
        }
    }

    /// Adds a new connection to the server. If a connection already exits it does nothing.
    pub fn add_connection(&mut self, connection_id: &C) {
        if self.connections.contains_key(connection_id) {
            return;
        }

        let connection = RemoteConnection::new(self.connection_config.clone());
        self.connections.insert(*connection_id, connection);
    }

    /// Returns whether or not the server has connections
    pub fn has_connections(&self) -> bool {
        !self.connections.is_empty()
    }

    pub fn disconnected_client(&mut self) -> Option<(C, DisconnectionReason)> {
        self.disconnections.pop()
    }

    pub fn network_info(&self, connection_id: C) -> Option<&NetworkInfo> {
        if let Some(connection) = self.connections.get(&connection_id) {
            return Some(connection.network_info());
        }
        None
    }

    /// Similar to disconnect but does not emit an event
    pub fn remove_connection(&mut self, connection_id: &C) {
        self.connections.remove(connection_id);
    }

    pub fn disconnect(&mut self, connection_id: &C) {
        if self.connections.remove(connection_id).is_some() {
            self.disconnections
                .push((*connection_id, DisconnectionReason::DisconnectedByServer));
        }
    }

    pub fn disconnect_all(&mut self) {
        for connection_id in self.connections_id().iter() {
            self.disconnect(connection_id);
        }
    }

    pub fn broadcast_message(&mut self, channel_id: u8, message: Vec<u8>) {
        for connection in self.connections.values_mut() {
            connection.send_message(channel_id, message.clone());
        }
    }

    pub fn broadcast_message_except(&mut self, except_id: &C, channel_id: u8, message: Vec<u8>) {
        for (connection_id, connection) in self.connections.iter_mut() {
            if except_id == connection_id {
                continue;
            }

            connection.send_message(channel_id, message.clone());
        }
    }

    pub fn send_message(&mut self, connection_id: &C, channel_id: u8, message: Vec<u8>) {
        match self.connections.get_mut(connection_id) {
            Some(connection) => connection.send_message(channel_id, message),
            None => log::error!("Tried to send message to disconnected client {:?}", connection_id),
        }
    }

    pub fn receive_message(&mut self, connection_id: &C, channel_id: u8) -> Option<Payload> {
        if let Some(connection) = self.connections.get_mut(connection_id) {
            return connection.receive_message(channel_id);
        }
        None
    }

    pub fn connections_id(&self) -> Vec<C> {
        self.connections.keys().copied().collect()
    }

    pub fn is_connected(&self, connection_id: &C) -> bool {
        self.connections.contains_key(connection_id)
    }

    pub fn update_connections(&mut self, duration: Duration) {
        for (&connection_id, connection) in self.connections.iter_mut() {
            connection.advance_time(duration);
            if connection.update().is_err() {
                let reason = connection.disconnected().unwrap();
                self.disconnections.push((connection_id, reason));
            }
        }
        self.connections.retain(|_, c| c.is_connected());
    }

    pub fn get_packets_to_send(&mut self, connection_id: &C) -> Result<Vec<Payload>, RechannelError> {
        match self.connections.get_mut(connection_id) {
            Some(connection) => connection.get_packets_to_send(),
            None => Err(RechannelError::ClientNotFound),
        }
    }

    pub fn process_packet_from(&mut self, payload: &[u8], connection_id: &C) -> Result<(), RechannelError> {
        match self.connections.get_mut(connection_id) {
            Some(connection) => connection.process_packet(payload),
            None => Err(RechannelError::ClientNotFound),
        }
    }
}
