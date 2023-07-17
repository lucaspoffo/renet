use std::time::Duration;

use super::MAX_MESSAGE_BATCH_SIZE;
use renet::RenetClient;
use steamworks::networking_sockets::NetworkingSockets;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection},
    networking_types::{NetConnectionEnd, NetworkingConnectionState, NetworkingIdentity, SendFlags},
    ClientManager, SteamId,
};

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct SteamClientTransport {
    networking_sockets: NetworkingSockets<ClientManager>,
    connection: NetConnection<ClientManager>,
}

impl SteamClientTransport {
    /// Create a new client connection to the server
    ///
    /// If the connection is not possible, it will return [`InvalidHandle`](steamworks::networking_sockets)
    pub fn new(client: &steamworks::Client<ClientManager>, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        let networking_sockets = client.networking_sockets();

        let options = Vec::new();
        let connection = client
            .networking_sockets()
            .connect_p2p(NetworkingIdentity::new_steam_id(*steam_id), 0, options)?;
        Ok(Self {
            networking_sockets,
            connection,
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
        if let Ok(info) = self.networking_sockets.get_connection_info(&self.connection) {
            if let Ok(state) = info.state() {
                return state;
            }
        }

        NetworkingConnectionState::None
    }

    pub fn disconnect_reason(&self) -> Option<NetConnectionEnd> {
        if let Ok(info) = self.networking_sockets.get_connection_info(&self.connection) {
            return info.end_reason();
        }

        None
    }

    pub fn client_id(&self, steam_client: &steamworks::Client<ClientManager>) -> u64 {
        steam_client.user().steam_id().raw()
    }

    pub fn disconnect(self, send_last_packets: bool) {
        self.connection.close(
            steamworks::networking_types::NetConnectionEnd::AppGeneric,
            Some("Disconnecting from server"),
            send_last_packets,
        );
    }

    pub fn update(&mut self, _duration: Duration, client: &mut RenetClient) {
        if !self.is_connected() {
            return;
        };

        let messages = self.connection.receive_messages(MAX_MESSAGE_BATCH_SIZE);
        messages.iter().for_each(|message| {
            client.process_packet(message.data());
        });
    }

    pub fn send_packets(&mut self, client: &mut RenetClient) {
        if !self.is_connected() {
            return;
        }

        let packets = client.get_packets_to_send();
        for packet in packets {
            if let Err(e) = self.connection.send_message(&packet, SendFlags::UNRELIABLE) {
                log::error!("Error while sending packet: {}", e);
            }
        }
        match self.connection.flush_messages() {
            Err(e) => log::error!("Error while flushing messages: {}", e),
            _ => (),
        }
    }
}
