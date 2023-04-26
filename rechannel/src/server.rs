use crate::error::{ClientNotFound, ConnectionError};
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, RemoteConnection};
use std::collections::HashMap;
use std::time::Duration;

use bytes::Bytes;

#[derive(Debug)]
pub struct RechannelServer {
    connections: HashMap<u64, RemoteConnection>,
    connection_config: ConnectionConfig,
}

impl RechannelServer {
    pub fn new(connection_config: ConnectionConfig) -> Self {
        Self {
            connections: HashMap::new(),
            connection_config,
        }
    }

    /// Adds a new connection to the server. If a connection already exits it does nothing.
    pub fn add_connection(&mut self, connection_id: u64) {
        if self.connections.contains_key(&connection_id) {
            return;
        }

        let connection = RemoteConnection::new(self.connection_config.clone());
        self.connections.insert(connection_id, connection);
    }

    /// Returns whether or not the server has connections
    pub fn has_connections(&self) -> bool {
        !self.connections.is_empty()
    }

    // pub fn client_rtt(&self, connection_id: C) -> f32 {
    //     match self.connections.get(&connection_id) {
    //         Some(connection) => connection.rtt(),
    //         None => 0.0,
    //     }
    // }

    // pub fn client_packet_loss(&self, connection_id: C) -> f32 {
    //     match self.connections.get(&connection_id) {
    //         Some(connection) => connection.packet_loss(),
    //         None => 0.0,
    //     }
    // }

    pub fn remove_connection(&mut self, connection_id: u64) {
        self.connections.remove(&connection_id);
    }

    pub fn disconnect(&mut self, connection_id: u64) {
        if let Some(connection) = self.connections.get_mut(&connection_id) {
            if !connection.has_error() {
                connection.error = Some(ConnectionError::DisconnectedByServer);
            }
        }
    }

    pub fn disconnect_all(&mut self) {
        for connection in self.connections.values_mut() {
            if !connection.has_error() {
                connection.error = Some(ConnectionError::DisconnectedByServer);
            }
        }
    }

    pub fn broadcast_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        let channel_id = channel_id.into();
        let message = message.into();
        for connection in self.connections.values_mut() {
            connection.send_message(channel_id, message.clone());
        }
    }

    pub fn broadcast_message_except<I: Into<u8>, B: Into<Bytes>>(&mut self, except_id: u64, channel_id: I, message: B) {
        let channel_id = channel_id.into();
        let message = message.into();
        for (connection_id, connection) in self.connections.iter_mut() {
            if except_id == *connection_id {
                continue;
            }

            connection.send_message(channel_id, message.clone());
        }
    }

    // pub fn can_send_message<I: Into<u8>>(&self, connection_id: &C, channel_id: I) -> bool {
    //     match self.connections.get(connection_id) {
    //         Some(connection) => connection.can_send_message(channel_id),
    //         None => false,
    //     }
    // }

    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, connection_id: u64, channel_id: I, message: B) {
        match self.connections.get_mut(&connection_id) {
            Some(connection) => connection.send_message(channel_id, message),
            None => log::error!("Tried to send a message to invalid client {:?}", connection_id),
        }
    }

    pub fn receive_message<I: Into<u8>>(&mut self, connection_id: u64, channel_id: I) -> Option<Bytes> {
        if let Some(connection) = self.connections.get_mut(&connection_id) {
            return connection.receive_message(channel_id);
        }
        None
    }

    pub fn connections_id(&self) -> Vec<u64> {
        self.connections.iter().filter(|(_, c)| !c.has_error()).map(|(id, _)| *id).collect()
    }

    pub fn disconnections_id(&self) -> Vec<u64> {
        self.connections.iter().filter(|(_, c)| c.has_error()).map(|(id, _)| *id).collect()
    }

    pub fn is_connected(&self, connection_id: u64) -> bool {
        self.connections.contains_key(&connection_id)
    }

    pub fn advance_time(&mut self, duration: Duration) {
        for connection in self.connections.values_mut() {
            connection.advance_time(duration);
        }
    }

    pub fn get_packets_to_send(&mut self, connection_id: u64) -> Result<Vec<Payload>, ClientNotFound> {
        match self.connections.get_mut(&connection_id) {
            Some(connection) => Ok(connection.get_packets_to_send()),
            None => Err(ClientNotFound),
        }
    }

    pub fn process_packet_from(&mut self, payload: &[u8], connection_id: u64) -> Result<(), ClientNotFound> {
        match self.connections.get_mut(&connection_id) {
            Some(connection) => Ok(connection.process_packet(payload)),
            None => Err(ClientNotFound),
        }
    }
}
