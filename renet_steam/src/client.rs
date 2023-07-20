use super::MAX_MESSAGE_BATCH_SIZE;
use renet::RenetClient;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection, NetworkingSockets},
    networking_types::{NetConnectionEnd, NetworkingConnectionState, NetworkingIdentity, SendFlags},
    ClientManager, SteamError, SteamId,
};

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct SteamClientTransport {
    networking_sockets: NetworkingSockets<ClientManager>,
    connection: Option<NetConnection<ClientManager>>,
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
            connection: Some(connection),
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
        let Some(connection) = &self.connection else {
            return NetworkingConnectionState::ClosedByPeer;
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
        let Some(connection) = &self.connection else {
            return Some(NetConnectionEnd::AppGeneric);
        };

        if let Ok(info) = self.networking_sockets.get_connection_info(connection) {
            return info.end_reason();
        }

        None
    }

    pub fn client_id(&self, steam_client: &steamworks::Client<ClientManager>) -> u64 {
        steam_client.user().steam_id().raw()
    }

    pub fn disconnect(&mut self, send_last_packets: bool) {
        if let Some(connection) = self.connection.take() {
            connection.close(NetConnectionEnd::AppGeneric, Some("Disconnecting from server"), send_last_packets);
        }
    }

    pub fn update(&mut self, client: &mut RenetClient) {
        if self.is_disconnected() {
            if self.connection.is_some() {
                self.disconnect(false);
            }
            return;
        };

        let connection = self.connection.as_mut().unwrap();
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

        let connection = self.connection.as_mut().unwrap();
        let packets = client.get_packets_to_send();
        for packet in packets {
            connection.send_message(&packet, SendFlags::UNRELIABLE)?;
        }

        connection.flush_messages()
    }
}
