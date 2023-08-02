use super::MAX_MESSAGE_BATCH_SIZE;
use renet::RenetClient;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection, NetworkingSockets},
    networking_types::{NetConnectionEnd, NetworkingConnectionState, NetworkingIdentity, SendFlags},
    ClientManager, SteamError, SteamId,
};

enum ConnectionState {
    Connected { connection: NetConnection<ClientManager> },
    Disconnected { end_reason: NetConnectionEnd },
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct SteamClientTransport {
    networking_sockets: NetworkingSockets<ClientManager>,
    state: ConnectionState,
}

impl SteamClientTransport {
    pub fn new(client: &steamworks::Client<ClientManager>, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        let networking_sockets = client.networking_sockets();

        let options = Vec::new();
        let connection = client
            .networking_sockets()
            .connect_p2p(NetworkingIdentity::new_steam_id(*steam_id), 0, options)?;
        Ok(Self {
            networking_sockets,
            state: ConnectionState::Connected { connection },
        })
    }

    pub fn is_connected(&self) -> bool {
        let status = self.connection_state();

        status == NetworkingConnectionState::Connected
    }

    pub fn is_disconnected(&self) -> bool {
        let status = self.connection_state();
        status == NetworkingConnectionState::ClosedByPeer
            || status == NetworkingConnectionState::ProblemDetectedLocally
            || status == NetworkingConnectionState::None
    }

    pub fn is_connecting(&self) -> bool {
        let status = self.connection_state();
        status == NetworkingConnectionState::Connecting || status == NetworkingConnectionState::FindingRoute
    }

    pub fn connection_state(&self) -> NetworkingConnectionState {
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

    pub fn client_id(&self, steam_client: &steamworks::Client<ClientManager>) -> u64 {
        steam_client.user().steam_id().raw()
    }

    pub fn disconnect(&mut self) {
        if matches!(self.state, ConnectionState::Disconnected { .. }) {
            return;
        }

        let disconnect_state = ConnectionState::Disconnected {
            end_reason: NetConnectionEnd::AppGeneric,
        };
        let old_state = std::mem::replace(&mut self.state, disconnect_state);
        if let ConnectionState::Connected { connection } = old_state {
            connection.close(NetConnectionEnd::AppGeneric, Some("Client disconnected"), false);
        }
    }

    pub fn update(&mut self, client: &mut RenetClient) {
        if self.is_disconnected() {
            // Mark the client as disconnected if an error occured in the transport layer
            if !client.is_disconnected() {
                client.disconnect_due_to_transport();
            }

            if let ConnectionState::Connected { connection } = &self.state {
                let end_reason = self
                    .networking_sockets
                    .get_connection_info(connection)
                    .map(|info| info.end_reason())
                    .unwrap_or_default()
                    .unwrap_or(NetConnectionEnd::AppGeneric);

                self.state = ConnectionState::Disconnected { end_reason };
            }

            return;
        };

        let ConnectionState::Connected { connection } = &mut self.state else {
            unreachable!()
        };

        let messages = connection.receive_messages(MAX_MESSAGE_BATCH_SIZE);
        messages.iter().for_each(|message| {
            client.process_packet(message.data());
        });
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
