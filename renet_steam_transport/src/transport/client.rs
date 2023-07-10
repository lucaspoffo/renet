use std::{sync::Arc, time::Duration};

use super::{Transport, MAX_MESSAGE_BATCH_SIZE};
use bevy_utils::syncunsafecell::SyncUnsafeCell;
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
    callback_handle: Option<CallbackHandle<ClientManager>>,
    disconnect_reason: NetConnectionEnd,
    pending: Arc<SyncUnsafeCell<Vec<NetConnectionStatusChanged>>>,
}

impl SteamClientTransport {
    /// Create a new client connection to the server
    ///
    /// If the connection is not possible, it will return [`InvalidHandle`](steamworks::networking_sockets)
    pub fn new(steam_client: &steamworks::Client<ClientManager>, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        let options = Vec::new();
        let connection = match steam_client
            .networking_sockets()
            .connect_p2p(NetworkingIdentity::new_steam_id(*steam_id), 0, options)
        {
            Ok(connection) => Ok(connection),
            Err(h) => Err(h),
        }?;
        Ok(Self {
            connection,
            status: NetworkingConnectionState::Connecting,
            callback_handle: None,
            disconnect_reason: NetConnectionEnd::AppGeneric,
            pending: Arc::new(SyncUnsafeCell::new(Vec::new())),
        })
    }

    /// Register the callback for the connection status so that the connection status can be updated
    pub fn register_callback(&mut self, steam_client: &steamworks::Client<ClientManager>) {
        let pending_in = Arc::clone(&self.pending);
        self.callback_handle = Some(
            steam_client.register_callback::<NetConnectionStatusChanged, _>(move |event| unsafe {
                (&mut *pending_in.get()).push(event);
            }),
        );
    }

    /// Handle the callbacks that have been received since the last call to this function
    /// This function should be called regularly to ensure that the connection status is up to date
    /// But it should be called after the steam_client has run its callbacks
    /// It will also update the disconnect reason if the connection has been closed
    pub fn handle_callbacks(&mut self) {
        let pending = unsafe { &mut *self.pending.get() };
        for event in pending.drain(..) {
            self.handle_callback(event);
        }
    }

    pub(crate) fn handle_callback(&mut self, event: NetConnectionStatusChanged) {
        self.status = match event.connection_info.state() {
            Ok(state) => state,
            Err(e) => {
                log::error!("Error while getting connection state: {}", e);
                NetworkingConnectionState::None
            }
        };
        if self.status == NetworkingConnectionState::ClosedByPeer || self.status == NetworkingConnectionState::ProblemDetectedLocally {
            self.disconnect_reason = event.connection_info.end_reason().unwrap_or(NetConnectionEnd::AppGeneric);
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

    pub fn disconnect_reason(&self) -> NetConnectionEnd {
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
        self.callback_handle.map(|handle| handle.disconnect());
    }
}

impl Transport<RenetClient> for SteamClientTransport {
    fn update(&mut self, _duration: Duration, client: &mut RenetClient) {
        let messages = self.connection.receive_messages(MAX_MESSAGE_BATCH_SIZE);
        messages.iter().for_each(|message| {
            client.process_packet(message.data());
        });
    }

    fn send_packets(&mut self, client: &mut RenetClient) {
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
