use crate::error::{DisconnectionReason, RechannelError};
use crate::network_info::NetworkInfo;
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, RemoteConnection};
use crate::transport::ServerTransport;

use std::collections::HashMap;
use std::time::Duration;

use bytes::Bytes;

/// Events that can occur in the server.
#[derive(Debug, Clone, Copy)]
pub enum ServerEvent {
    ClientConnected { client_id: u64 },
    ClientDisconnected { client_id: u64, reason: DisconnectionReason },
}

#[derive(Debug)]
pub struct RechannelServer {
    clients: HashMap<u64, RemoteConnection>,
    connection_config: ConnectionConfig,
    events: Vec<ServerEvent>,
}

impl RechannelServer {
    pub fn new(connection_config: ConnectionConfig) -> Self {
        connection_config
            .fragment_config
            .assert_can_fragment_packet_with_size(connection_config.max_packet_size);

        Self {
            clients: HashMap::new(),
            connection_config,
            events: Vec::new(),
        }
    }

    /// Adds a new connection to the server. If a connection already exits it does nothing.
    pub fn add_client(&mut self, client_id: u64) {
        if self.clients.contains_key(&client_id) {
            return;
        }

        let connection = RemoteConnection::new(self.connection_config.clone());
        self.clients.insert(client_id, connection);
        self.events.push(ServerEvent::ClientConnected { client_id })
    }

    /// Returns whether or not the server has connections
    pub fn has_clients(&self) -> bool {
        !self.clients.is_empty()
    }

    pub fn events(&self) -> impl Iterator<Item = &ServerEvent> {
        self.events.iter()
    }

    pub fn network_info(&self, client_id: u64) -> Option<NetworkInfo> {
        let connection = self.clients.get(&client_id)?;
        Some(connection.network_info())
    }

    /// Similar to disconnect but does not emit an event
    pub fn remove_connection(&mut self, client_id: u64) {
        self.clients.remove(&client_id);
    }

    pub fn disconnect(&mut self, client_id: u64) {
        if self.clients.remove(&client_id).is_some() {
            self.events.push(ServerEvent::ClientDisconnected {
                client_id,
                reason: DisconnectionReason::DisconnectedByServer,
            });
        }
    }

    pub fn disconnect_with_reason(&mut self, client_id: u64, reason: DisconnectionReason) {
        if self.clients.remove(&client_id).is_some() {
            self.events.push(ServerEvent::ClientDisconnected { client_id, reason });
        }
    }

    pub fn disconnect_all(&mut self) {
        for client_id in self.clients_id() {
            self.disconnect(client_id);
        }
    }

    pub fn broadcast_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        let channel_id = channel_id.into();
        let message = message.into();
        for connection in self.clients.values_mut() {
            connection.send_message(channel_id, message.clone());
        }
    }

    pub fn broadcast_message_except<I: Into<u8>, B: Into<Bytes>>(&mut self, except_id: u64, channel_id: I, message: B) {
        let channel_id = channel_id.into();
        let message = message.into();
        for (connection_id, connection) in self.clients.iter_mut() {
            if except_id == *connection_id {
                continue;
            }

            connection.send_message(channel_id, message.clone());
        }
    }

    pub fn can_send_message<I: Into<u8>>(&self, client_id: u64, channel_id: I) -> bool {
        match self.clients.get(&client_id) {
            Some(connection) => connection.can_send_message(channel_id),
            None => false,
        }
    }

    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, client_id: u64, channel_id: I, message: B) {
        match self.clients.get_mut(&client_id) {
            Some(connection) => connection.send_message(channel_id, message),
            None => log::error!("Tried to send message to disconnected client {:?}", client_id),
        }
    }

    pub fn receive_message<I: Into<u8>>(&mut self, client_id: u64, channel_id: I) -> Option<Payload> {
        if let Some(connection) = self.clients.get_mut(&client_id) {
            return connection.receive_message(channel_id);
        }
        None
    }

    pub fn clients_id(&self) -> Vec<u64> {
        self.clients.keys().copied().collect()
    }

    pub fn is_connected(&self, client_id: u64) -> bool {
        self.clients.contains_key(&client_id)
    }

    pub fn update(&mut self, duration: Duration) {
        self.events.clear();

        for (&client_id, connection) in self.clients.iter_mut() {
            if connection.update(duration).is_err() {
                let reason = connection.disconnected().unwrap();
                self.events.push(ServerEvent::ClientDisconnected { client_id, reason });
            }
        }

        self.clients.retain(|_, c| c.is_connected());
    }

    pub fn get_packets_to_send(&mut self, client_id: u64) -> Result<Vec<Payload>, RechannelError> {
        match self.clients.get_mut(&client_id) {
            Some(connection) => connection.get_packets_to_send(),
            None => Err(RechannelError::ClientNotFound),
        }
    }

    pub fn process_packet_from(&mut self, payload: &[u8], client_id: u64) -> Result<(), RechannelError> {
        match self.clients.get_mut(&client_id) {
            Some(connection) => connection.process_packet(payload),
            None => Err(RechannelError::ClientNotFound),
        }
    }

    pub fn send_packets(&mut self, transport: &mut dyn ServerTransport) {
        for client_id in self.clients_id().into_iter() {
            let packets = match self.get_packets_to_send(client_id) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Failed to get packets from {}: {}", client_id, e);
                    continue;
                }
            };

            for packet in packets.iter() {
                transport.send_to(client_id, packet);
            }
        }
    }
}
