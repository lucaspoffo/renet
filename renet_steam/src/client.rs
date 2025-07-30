use std::net::SocketAddr;

use super::DEFAULT_MAX_MESSAGE_BATCH_SIZE;
use log::info;
use renet::RenetClient;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection, NetworkingSockets},
    networking_types::{NetConnectionEnd, NetworkingConfigEntry, NetworkingConnectionState, NetworkingIdentity, SendFlags},
    SteamError, SteamId,
};

enum ConnectionState {
    Connected { connection: NetConnection },
    Disconnected { end_reason: NetConnectionEnd },
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::resource::Resource))]
pub struct SteamClientTransport {
    networking_sockets: NetworkingSockets,
    state: ConnectionState,
    max_batch_size: usize,
}

#[derive(Clone)]
pub struct SteamClientTransportConfig {
    configs: Vec<NetworkingConfigEntry>,
    max_batch_size: usize,
}

impl Default for SteamClientTransportConfig {
    fn default() -> Self {
        Self {
            configs: vec![],
            max_batch_size: DEFAULT_MAX_MESSAGE_BATCH_SIZE,
        }
    }
}

impl SteamClientTransportConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(mut self, config: NetworkingConfigEntry) -> Self {
        self.configs.push(config);
        self
    }

    pub fn with_max_batch_size(mut self, max_batch_size: usize) -> Self {
        self.max_batch_size = max_batch_size;
        self
    }
}

impl SteamClientTransport {
    pub fn new_p2p(client: &steamworks::Client, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        Self::new_p2p_with_config(client, steam_id, Default::default())
    }

    pub fn new_ip(client: &steamworks::Client, socket_addr: SocketAddr) -> Result<Self, InvalidHandle> {
        Self::new_ip_with_config(client, socket_addr, Default::default())
    }

    pub fn new_p2p_with_config(
        client: &steamworks::Client,
        steam_id: &SteamId,
        config: SteamClientTransportConfig,
    ) -> Result<Self, InvalidHandle> {
        let networking_sockets = client.networking_sockets();

        let connection = networking_sockets.connect_p2p(NetworkingIdentity::new_steam_id(*steam_id), 0, config.configs)?;
        Ok(Self {
            networking_sockets,
            state: ConnectionState::Connected { connection },
            max_batch_size: config.max_batch_size,
        })
    }

    pub fn new_ip_with_config(
        client: &steamworks::Client,
        socket_addr: SocketAddr,
        config: SteamClientTransportConfig,
    ) -> Result<Self, InvalidHandle> {
        let networking_sockets = client.networking_sockets();

        let connection = networking_sockets.connect_by_ip_address(socket_addr, config.configs)?;
        Ok(Self {
            networking_sockets,
            state: ConnectionState::Connected { connection },
            max_batch_size: config.max_batch_size,
        })
    }

    fn is_connected(&self) -> bool {
        let status = self.connection_state();

        status == NetworkingConnectionState::Connected
    }

    fn is_disconnected(&self) -> bool {
        let status = self.connection_state();
        status == NetworkingConnectionState::ClosedByPeer
            || status == NetworkingConnectionState::ProblemDetectedLocally
            || status == NetworkingConnectionState::None
    }

    fn is_connecting(&self) -> bool {
        let status = self.connection_state();
        status == NetworkingConnectionState::Connecting || status == NetworkingConnectionState::FindingRoute
    }

    fn connection_state(&self) -> NetworkingConnectionState {
        let connection = match &self.state {
            ConnectionState::Connected { connection } => connection,
            ConnectionState::Disconnected { .. } => {
                return NetworkingConnectionState::None;
            }
        };

        let Ok(info) = self.networking_sockets.get_connection_info(connection) else {
            return NetworkingConnectionState::None;
        };

        match info.state() {
            Ok(state) => state,
            Err(_) => NetworkingConnectionState::None,
        }
    }

    pub fn disconnect_reason(&self) -> Option<NetConnectionEnd> {
        let connection = match &self.state {
            ConnectionState::Connected { connection } => connection,
            ConnectionState::Disconnected { end_reason, .. } => {
                return Some(*end_reason);
            }
        };

        if let Ok(info) = self.networking_sockets.get_connection_info(connection) {
            return info.end_reason();
        }

        None
    }

    pub fn client_id(&self, steam_client: &steamworks::Client) -> u64 {
        steam_client.user().steam_id().raw()
    }

    pub fn disconnect(&mut self) {
        info!("Disconnect called!");
        if matches!(self.state, ConnectionState::Disconnected { .. }) {
            return;
        }

        let disconnect_state = ConnectionState::Disconnected {
            end_reason: NetConnectionEnd::MiscGeneric,
        };
        let old_state = std::mem::replace(&mut self.state, disconnect_state);
        if let ConnectionState::Connected { connection } = old_state {
            connection.close(NetConnectionEnd::MiscGeneric, Some("Client disconnected"), false);
        }
    }

    pub fn update(&mut self, client: &mut RenetClient) {
        if self.is_disconnected() {
            info!("Mark DC called!");
            // Mark the client as disconnected if an error occured in the transport layer
            client.disconnect_due_to_transport();

            if let ConnectionState::Connected { connection } = &self.state {
                let end_reason = self
                    .networking_sockets
                    .get_connection_info(connection)
                    .map(|info| info.end_reason())
                    .unwrap_or_default()
                    .unwrap_or(NetConnectionEnd::MiscGeneric);

                self.state = ConnectionState::Disconnected { end_reason };
            }

            return;
        };

        if self.is_connected() {
            client.set_connected();
        } else if self.is_connecting() {
            client.set_connecting();
        }

        let ConnectionState::Connected { connection } = &mut self.state else {
            unreachable!()
        };

        match connection.receive_messages(DEFAULT_MAX_MESSAGE_BATCH_SIZE) {
            Ok(messages) => {
                println!("N MESSAGES: {}", messages.len());
                messages.iter().for_each(|message| {
                    client.process_packet(message.data());
                });
            }
            Err(e) => {
                log::error!("Client Message Receive Error: {e:?}");
            }
        }
    }

    pub fn send_packets(&mut self, client: &mut RenetClient) -> Result<(), SteamError> {
        if self.is_disconnected() {
            return Err(SteamError::NoConnection);
        }

        if self.is_connecting() {
            return Ok(());
        }

        let ConnectionState::Connected { connection } = &mut self.state else {
            unreachable!()
        };
        let packets = client.get_packets_to_send();
        for packet in packets {
            connection.send_message(&packet, SendFlags::UNRELIABLE)?;
        }

        connection.flush_messages()
    }
}
