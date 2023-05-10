use crate::error::{ClientNotFound, DisconnectReason};
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RenetClient};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use bytes::Bytes;

#[derive(Debug)]
pub enum ServerEvent {
    ClientConnected { client_id: u64 },
    ClientDisconnected { client_id: u64, reason: DisconnectReason },
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct RenetServer {
    connections: HashMap<u64, RenetClient>,
    connection_config: ConnectionConfig,
    events: VecDeque<ServerEvent>,
}

impl RenetServer {
    pub fn new(connection_config: ConnectionConfig) -> Self {
        Self {
            connections: HashMap::new(),
            connection_config,
            events: VecDeque::new(),
        }
    }

    /// Adds a new connection to the server. If a connection already exits it does nothing.
    pub fn add_connection(&mut self, connection_id: u64) {
        if self.connections.contains_key(&connection_id) {
            return;
        }

        let connection = RenetClient::new_from_server(self.connection_config.clone());
        self.connections.insert(connection_id, connection);
        self.events.push_back(ServerEvent::ClientConnected { client_id: connection_id })
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    /// Returns whether or not the server has connections
    pub fn has_connections(&self) -> bool {
        !self.connections.is_empty()
    }

    pub fn disconnect_reason(&self, connection_id: u64) -> Option<DisconnectReason> {
        if let Some(connection) = self.connections.get(&connection_id) {
            return connection.disconnect_reason();
        }

        None
    }

    pub fn rtt(&self, connection_id: u64) -> f64 {
        match self.connections.get(&connection_id) {
            Some(connection) => connection.rtt(),
            None => 0.0,
        }
    }

    pub fn packet_loss(&self, connection_id: u64) -> f64 {
        match self.connections.get(&connection_id) {
            Some(connection) => connection.packet_loss(),
            None => 0.0,
        }
    }

    pub fn bytes_sent_per_sec(&self, connection_id: u64) -> f64 {
        match self.connections.get(&connection_id) {
            Some(connection) => connection.bytes_sent_per_sec(),
            None => 0.0,
        }
    }

    pub fn bytes_received_per_sec(&self, connection_id: u64) -> f64 {
        match self.connections.get(&connection_id) {
            Some(connection) => connection.bytes_received_per_sec(),
            None => 0.0,
        }
    }

    pub fn network_info(&self, connection_id: u64) -> Result<NetworkInfo, ClientNotFound> {
        match self.connections.get(&connection_id) {
            Some(connection) => Ok(connection.network_info()),
            None => Err(ClientNotFound),
        }
    }

    pub fn remove_connection(&mut self, connection_id: u64) {
        if let Some(connection) = self.connections.remove(&connection_id) {
            let reason = connection.disconnect_reason().unwrap_or(DisconnectReason::Transport);
            self.events.push_back(ServerEvent::ClientDisconnected {
                client_id: connection_id,
                reason,
            });
        }
    }

    pub fn disconnect(&mut self, connection_id: u64) {
        if let Some(connection) = self.connections.get_mut(&connection_id) {
            if connection.is_connected() {
                connection.disconnect_reason = Some(DisconnectReason::DisconnectedByServer);
            }
        }
    }

    pub fn disconnect_all(&mut self) {
        for connection in self.connections.values_mut() {
            if connection.is_connected() {
                connection.disconnect_reason = Some(DisconnectReason::DisconnectedByServer);
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
        self.connections
            .iter()
            .filter(|(_, c)| !c.is_disconnected())
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn disconnections_id(&self) -> Vec<u64> {
        self.connections
            .iter()
            .filter(|(_, c)| c.is_disconnected())
            .map(|(id, _)| *id)
            .collect()
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
            Some(connection) => {
                connection.process_packet(payload);
                Ok(())
            }
            None => Err(ClientNotFound),
        }
    }
}
