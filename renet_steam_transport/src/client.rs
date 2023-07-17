use std::{time::Duration, sync::mpsc::Receiver};
use std::sync::mpsc;

use super::MAX_MESSAGE_BATCH_SIZE;
use renet::RenetClient;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection},
    networking_types::{NetConnectionEnd, NetConnectionStatusChanged, NetworkingConnectionState, NetworkingIdentity, SendFlags},
    CallbackHandle, ClientManager, SteamId,
};
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct SteamClientTransport {
    connection: NetConnection<ClientManager>,
    status: NetworkingConnectionState,
    _callback_handle: CallbackHandle<ClientManager>,
    disconnect_reason: Option<NetConnectionEnd>,
    status_change_receiver: Receiver<NetConnectionStatusChanged>,
}

impl SteamClientTransport {
    /// Create a new client connection to the server
    ///
    /// If the connection is not possible, it will return [`InvalidHandle`](steamworks::networking_sockets)
    pub fn new(client: &steamworks::Client<ClientManager>, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        let options = Vec::new();
        // it shouldnt be the case here to buffer more then 2 events, but just in case
        let (sender, status_change_receiver) = mpsc::channel();
        let _callback_handle = client.register_callback::<NetConnectionStatusChanged, _>(move |event| {
            if let Err(e) = sender.send(event) {
                log::error!("Failed to send connection status change: {e}");
            }
        });
        let connection = client
            .networking_sockets()
            .connect_p2p(NetworkingIdentity::new_steam_id(*steam_id), 0, options)?;
        Ok(Self {
            connection,
            status: NetworkingConnectionState::Connecting,
            _callback_handle,
            disconnect_reason: None,
            status_change_receiver,
        })
    }

    fn handle_callback(&mut self, event: NetConnectionStatusChanged) {
        self.status = match event.connection_info.state() {
            Ok(state) => state,
            Err(e) => {
                log::error!("Error while getting connection state: {}", e);
                NetworkingConnectionState::None
            }
        };
        if self.status == NetworkingConnectionState::ClosedByPeer || self.status == NetworkingConnectionState::ProblemDetectedLocally {
            self.disconnect_reason = event.connection_info.end_reason();
        }
    }

    pub fn is_connected(&self) -> bool {
        self.status == NetworkingConnectionState::Connected
    }

    pub fn is_disconnected(&self) -> bool {
        self.status == NetworkingConnectionState::ClosedByPeer
            || self.status == NetworkingConnectionState::ProblemDetectedLocally
            || self.status == NetworkingConnectionState::None
    }

    pub fn is_connecting(&self) -> bool {
        self.status == NetworkingConnectionState::Connecting || self.status == NetworkingConnectionState::FindingRoute
    }

    pub fn get_connection_state(&self) -> NetworkingConnectionState {
        self.status
    }

    pub fn disconnect_reason(&self) -> Option<NetConnectionEnd> {
        self.disconnect_reason
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
        if let Ok(event) = self.status_change_receiver.try_recv() {
            self.handle_callback(event);
        }

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
