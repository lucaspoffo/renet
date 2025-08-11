use super::MAX_MESSAGE_BATCH_SIZE;
use renet::RenetClient;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection},
    networking_types::{AppNetConnectionEnd, NetConnectionEnd, NetworkingConnectionState, NetworkingIdentity, SendFlags},
    Client, SteamError, SteamId,
};

enum ConnectionState {
    Connected { connection: NetConnection },
    Disconnected { end_reason: NetConnectionEnd },
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::resource::Resource))]
pub struct SteamClientTransport {
    client: Client,
    state: ConnectionState,
}

impl SteamClientTransport {
    pub fn new(client: Client, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        let options = Vec::new();
        let connection = client
            .networking_sockets()
            .connect_p2p(NetworkingIdentity::new_steam_id(*steam_id), 0, options)?;
        Ok(Self {
            client,
            state: ConnectionState::Connected { connection },
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

        let Ok(info) = self.client.networking_sockets().get_connection_info(connection) else {
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

        if let Ok(info) = self.client.networking_sockets().get_connection_info(connection) {
            return info.end_reason();
        }

        None
    }

    pub fn client_id(&self, steam_client: &steamworks::Client) -> u64 {
        steam_client.user().steam_id().raw()
    }

    pub fn disconnect(&mut self) {
        if matches!(self.state, ConnectionState::Disconnected { .. }) {
            return;
        }

        let disconnect_state = ConnectionState::Disconnected {
            end_reason: NetConnectionEnd::App(AppNetConnectionEnd::generic_normal()),
        };
        let old_state = std::mem::replace(&mut self.state, disconnect_state);
        if let ConnectionState::Connected { connection } = old_state {
            connection.close(
                NetConnectionEnd::App(AppNetConnectionEnd::generic_normal()),
                Some("Client disconnected"),
                false,
            );
        }
    }

    pub fn update(&mut self, client: &mut RenetClient) {
        if self.is_disconnected() {
            // Mark the client as disconnected if an error occured in the transport layer
            client.disconnect_due_to_transport();

            if let ConnectionState::Connected { connection } = &self.state {
                let end_reason = self
                    .client
                    .networking_sockets()
                    .get_connection_info(connection)
                    .map(|info| info.end_reason())
                    .unwrap_or_default()
                    .unwrap_or(NetConnectionEnd::App(AppNetConnectionEnd::generic_normal()));

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

        if let Ok(messages) = connection.receive_messages(MAX_MESSAGE_BATCH_SIZE) {
            messages.iter().for_each(|message| {
                client.process_packet(message.data());
            });
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
