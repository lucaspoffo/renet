use crate::error::{ClientNotFound, DisconnectReason};
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RenetClient};
use std::collections::{HashMap, VecDeque};
use std::fmt::Display;
use std::time::Duration;

use bytes::Bytes;

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct NetworkId(u64);

impl NetworkId {
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl From<u64> for NetworkId {
    fn from(value: u64) -> Self {
        Self::from_raw(value)
    }
}

impl From<&u64> for NetworkId {
    fn from(value: &u64) -> Self {
        Self::from_raw(*value)
    }
}

impl Display for NetworkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for NetworkId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for NetworkId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        u64::deserialize(deserializer).map(NetworkId::from_raw)
    }
}

/// Connection and disconnection events in the server.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Event))]
pub enum ServerEvent {
    ClientConnected { client_id: NetworkId },
    ClientDisconnected { client_id: NetworkId, reason: DisconnectReason },
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct RenetServer {
    connections: HashMap<NetworkId, RenetClient>,
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
    /// <p style="background:rgba(77,220,255,0.16);padding:0.5em;">
    /// <strong>Note:</strong> This should only be called by the transport layer.
    /// </p>
    pub fn add_connection(&mut self, client_id: NetworkId) {
        if self.connections.contains_key(&client_id) {
            return;
        }

        let connection = RenetClient::new_from_server(self.connection_config.clone());
        self.connections.insert(client_id, connection);
        self.events.push_back(ServerEvent::ClientConnected { client_id })
    }

    /// Returns a server event if available
    ///
    /// # Usage
    /// ```
    /// # use renet::{RenetServer, ConnectionConfig, ServerEvent};
    /// # let mut server = RenetServer::new(ConnectionConfig::default());
    /// while let Some(event) = server.get_event() {
    ///     match event {
    ///         ServerEvent::ClientConnected { client_id } => {
    ///             println!("Client {client_id} connected.")
    ///         }
    ///         ServerEvent::ClientDisconnected { client_id, reason } => {
    ///             println!("Client {client_id} disconnected: {reason}");
    ///         }
    ///     }
    /// }
    /// ```
    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    /// Returns whether or not the server has connections
    pub fn has_connections(&self) -> bool {
        !self.connections.is_empty()
    }

    /// Returns the disconnection reason for the client if its disconnected
    pub fn disconnect_reason(&self, client_id: NetworkId) -> Option<DisconnectReason> {
        if let Some(connection) = self.connections.get(&client_id) {
            return connection.disconnect_reason();
        }

        None
    }

    /// Returns the round-time trip for the client or 0.0 if the client is not found
    pub fn rtt(&self, client_id: NetworkId) -> f64 {
        match self.connections.get(&client_id) {
            Some(connection) => connection.rtt(),
            None => 0.0,
        }
    }

    /// Returns the packet loss for the client or 0.0 if the client is not found
    pub fn packet_loss(&self, client_id: NetworkId) -> f64 {
        match self.connections.get(&client_id) {
            Some(connection) => connection.packet_loss(),
            None => 0.0,
        }
    }

    /// Returns the bytes sent per seconds for the client or 0.0 if the client is not found
    pub fn bytes_sent_per_sec(&self, client_id: NetworkId) -> f64 {
        match self.connections.get(&client_id) {
            Some(connection) => connection.bytes_sent_per_sec(),
            None => 0.0,
        }
    }

    /// Returns the bytes received per seconds for the client or 0.0 if the client is not found
    pub fn bytes_received_per_sec(&self, client_id: NetworkId) -> f64 {
        match self.connections.get(&client_id) {
            Some(connection) => connection.bytes_received_per_sec(),
            None => 0.0,
        }
    }

    /// Returns all network informations for the client
    pub fn network_info(&self, client_id: NetworkId) -> Result<NetworkInfo, ClientNotFound> {
        match self.connections.get(&client_id) {
            Some(connection) => Ok(connection.network_info()),
            None => Err(ClientNotFound),
        }
    }

    /// Removes a connection from the server, emits an disconnect server event.
    /// It does nothing if the client does not exits.
    /// <p style="background:rgba(77,220,255,0.16);padding:0.5em;">
    /// <strong>Note:</strong> This should only be called by the transport layer.
    /// </p>
    pub fn remove_connection(&mut self, client_id: NetworkId) {
        if let Some(connection) = self.connections.remove(&client_id) {
            let reason = connection.disconnect_reason().unwrap_or(DisconnectReason::Transport);
            self.events.push_back(ServerEvent::ClientDisconnected { client_id, reason });
        }
    }

    /// Disconnects a client, it does nothing if the client does not exits.
    pub fn disconnect(&mut self, client_id: NetworkId) {
        if let Some(connection) = self.connections.get_mut(&client_id) {
            if !connection.is_disconnected() {
                connection.disconnect_reason = Some(DisconnectReason::DisconnectedByServer);
            }
        }
    }

    /// Disconnects all client.
    pub fn disconnect_all(&mut self) {
        for connection in self.connections.values_mut() {
            if !connection.is_disconnected() {
                connection.disconnect_reason = Some(DisconnectReason::DisconnectedByServer);
            }
        }
    }

    /// Send a message to all clients over a channel.
    pub fn broadcast_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        let channel_id = channel_id.into();
        let message = message.into();
        for connection in self.connections.values_mut() {
            connection.send_message(channel_id, message.clone());
        }
    }

    /// Send a message to all clients, except the specified one, over a channel.
    pub fn broadcast_message_except<I: Into<u8>, B: Into<Bytes>>(&mut self, except_id: NetworkId, channel_id: I, message: B) {
        let channel_id = channel_id.into();
        let message = message.into();
        for (connection_id, connection) in self.connections.iter_mut() {
            if except_id == *connection_id {
                continue;
            }

            connection.send_message(channel_id, message.clone());
        }
    }

    /// Returns the available memory in bytes of a channel for the given client.
    /// Returns 0 if the client is not found.
    pub fn channel_available_memory<I: Into<u8>>(&self, client_id: NetworkId, channel_id: I) -> usize {
        match self.connections.get(&client_id) {
            Some(connection) => connection.channel_available_memory(channel_id),
            None => 0,
        }
    }

    /// Checks if can send a message with the given size in bytes over a channel for the given client.
    /// Returns false if the client is not found.
    pub fn can_send_message<I: Into<u8>>(&self, client_id: NetworkId, channel_id: I, size_bytes: usize) -> bool {
        match self.connections.get(&client_id) {
            Some(connection) => connection.can_send_message(channel_id, size_bytes),
            None => false,
        }
    }

    /// Send a message to a client over a channel.
    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, client_id: NetworkId, channel_id: I, message: B) {
        match self.connections.get_mut(&client_id) {
            Some(connection) => connection.send_message(channel_id, message),
            None => log::error!("Tried to send a message to invalid client {:?}", client_id),
        }
    }

    /// Receive a message from a client over a channel.
    pub fn receive_message<I: Into<u8>>(&mut self, client_id: NetworkId, channel_id: I) -> Option<Bytes> {
        if let Some(connection) = self.connections.get_mut(&client_id) {
            return connection.receive_message(channel_id);
        }
        None
    }

    /// Return ids for all connected clients (iterator)
    pub fn clients_id_iter(&self) -> impl Iterator<Item = NetworkId> + '_ {
        self.connections.iter().filter(|(_, c)| !c.is_disconnected()).map(|(id, _)| *id)
    }

    /// Return ids for all connected clients
    pub fn clients_id(&self) -> Vec<NetworkId> {
        self.clients_id_iter().collect()
    }

    /// Return ids for all disconnected clients (iterator)
    pub fn disconnections_id_iter(&self) -> impl Iterator<Item = NetworkId> + '_ {
        self.connections.iter().filter(|(_, c)| c.is_disconnected()).map(|(id, _)| *id)
    }

    /// Return ids for all disconnected clients
    pub fn disconnections_id(&self) -> Vec<NetworkId> {
        self.disconnections_id_iter().collect()
    }

    /// Returns the current number of connected clients.
    pub fn connected_clients(&self) -> usize {
        self.connections.iter().filter(|(_, c)| c.is_disconnected()).count()
    }

    pub fn is_connected(&self, client_id: NetworkId) -> bool {
        if let Some(connection) = self.connections.get(&client_id) {
            return !connection.is_disconnected();
        }

        false
    }

    /// Advances the server by the duration.
    /// Should be called every tick
    pub fn update(&mut self, duration: Duration) {
        for connection in self.connections.values_mut() {
            connection.update(duration);
        }
    }

    /// Returns a list of packets to be sent to the client.
    /// <p style="background:rgba(77,220,255,0.16);padding:0.5em;">
    /// <strong>Note:</strong> This should only be called by the transport layer.
    /// </p>
    pub fn get_packets_to_send(&mut self, client_id: NetworkId) -> Result<Vec<Payload>, ClientNotFound> {
        match self.connections.get_mut(&client_id) {
            Some(connection) => Ok(connection.get_packets_to_send()),
            None => Err(ClientNotFound),
        }
    }

    /// Process a packet received from the client.
    /// <p style="background:rgba(77,220,255,0.16);padding:0.5em;">
    /// <strong>Note:</strong> This should only be called by the transport layer.
    /// </p>
    pub fn process_packet_from(&mut self, payload: &[u8], client_id: NetworkId) -> Result<(), ClientNotFound> {
        match self.connections.get_mut(&client_id) {
            Some(connection) => {
                connection.process_packet(payload);
                Ok(())
            }
            None => Err(ClientNotFound),
        }
    }
}
