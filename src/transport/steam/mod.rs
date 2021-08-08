use crate::connection_control::ConnectionControl;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::{Message, Payload};
use crate::transport::{TransportClient, TransportServer};

use bincode;
use crossbeam_channel::{unbounded, Receiver, TryRecvError};
use log::{error, warn};
use steamworks::{
    CallbackHandle, Client, ClientManager, P2PSessionConnectFail, P2PSessionRequest, SendType,
    SteamId,
};

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct SteamClient {
    server_id: SteamId,
    client: Client<ClientManager>,
    disconnect_reason: Option<DisconnectionReason>,
    connection_fail: Arc<AtomicBool>,
    _session_request: CallbackHandle<ClientManager>,
    _session_disconnect: CallbackHandle<ClientManager>,
}

impl SteamClient {
    pub fn new(client: Client<ClientManager>, server_id: SteamId) -> Self {
        let client_p2p = client.clone();
        let _session_request = client.register_callback(move |request: P2PSessionRequest| {
            if request.remote == server_id {
                client_p2p.networking().accept_p2p_session(request.remote);
            } else {
                warn!(
                    "Rejected connection request from unknow source: {:?}",
                    request.remote
                );
            }
        });

        let connection_fail = Arc::new(AtomicBool::new(false));
        let fail = connection_fail.clone();

        let _session_disconnect =
            client.register_callback(move |request: P2PSessionConnectFail| {
                if request.remote == server_id {
                    fail.store(true, Ordering::Relaxed);
                }
            });

        Self {
            server_id,
            client,
            disconnect_reason: None,
            connection_fail,
            _session_disconnect,
            _session_request,
        }
    }
}

impl TransportClient for SteamClient {
    fn recv(&mut self) -> Result<Option<Payload>, RenetError> {
        let mut read_buf = [0u8; 1200];
        while let Some(size) = self.client.networking().is_p2p_packet_available() {
            if size >= read_buf.len() {
                self.client
                    .networking()
                    .read_p2p_packet(read_buf.as_mut())
                    .unwrap();
                warn!("Discarted packet, size was above the limit: {}", size);

                continue;
            }

            let (remote, len) = self
                .client
                .networking()
                .read_p2p_packet(read_buf.as_mut())
                .unwrap();
            if self.server_id == remote {
                return Ok(Some(read_buf[..len].to_vec()));
            } else {
                warn!("Discarted packet from unknow source");
            }
        }

        Ok(None)
    }

    fn update(&mut self) {
        if self.disconnect_reason.is_none() && self.connection_fail.load(Ordering::Relaxed) {
            self.disconnect_reason = Some(DisconnectionReason::TransportError);
        }
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        self.disconnect_reason
    }

    fn send_to_server(&mut self, payload: &[u8]) {
        if !self
            .client
            .networking()
            .send_p2p_packet(self.server_id, SendType::Unreliable, payload)
        {
            error!("Error sending packet to server");
        }
    }

    fn disconnect(&mut self, reason: DisconnectionReason) {
        self.disconnect_reason = Some(reason);
        self.client.networking().close_p2p_session(self.server_id);
    }

    fn is_connected(&self) -> bool {
        self.disconnect_reason.is_none()
    }
}

pub struct SteamServer {
    server_id: SteamId,
    client: Client<ClientManager>,
    connecting_clients: Receiver<SteamId>,
    disconnected_clients: Receiver<SteamId>,
    // TODO: we can use Receiver and move the pooling to another thread.
    // received_packets: Receiver<(SteamId, Payload)>,
    connected_clients: HashSet<SteamId>,
    _session_request: CallbackHandle<ClientManager>,
    _session_disconnect: CallbackHandle<ClientManager>,
}

impl SteamServer {
    pub fn new(server_id: SteamId, client: Client<ClientManager>) -> Self {
        let (send_connecting, connecting_clients) = unbounded();
        let (send_disconnected, disconnected_clients) = unbounded();

        let _session_request = client.register_callback(move |request: P2PSessionRequest| {
            // TODO(error): unwrap
            send_connecting.send(request.remote).unwrap();
        });

        let _session_disconnect =
            client.register_callback(move |request: P2PSessionConnectFail| {
                // TODO(error): unwrap
                send_disconnected.send(request.remote).unwrap();
            });

        Self {
            server_id,
            client,
            connecting_clients,
            disconnected_clients,
            connected_clients: HashSet::new(),
            _session_request,
            _session_disconnect,
        }
    }
}

impl TransportServer for SteamServer {
    type ClientId = SteamId;
    type ConnectionId = SteamId;

    fn recv(
        &mut self,
        _connection_control: &ConnectionControl<Self::ClientId>,
    ) -> Result<Option<(Self::ClientId, Payload)>, Box<dyn std::error::Error + Send + Sync + 'static>>
    {
        let mut read_buf = [0u8; 1200];
        while let Some(size) = self.client.networking().is_p2p_packet_available() {
            if size >= read_buf.len() {
                self.client
                    .networking()
                    .read_p2p_packet(read_buf.as_mut())
                    .unwrap();
                error!("Discarted packet, size was above the limit");

                continue;
            }

            let (remote, len) = self
                .client
                .networking()
                .read_p2p_packet(read_buf.as_mut())
                .unwrap();
            if self.connected_clients.contains(&remote) {
                return Ok(Some((remote, read_buf[..len].to_vec())));
            }
        }

        Ok(None)
    }

    fn send(
        &mut self,
        client_id: SteamId,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        if !self
            .client
            .networking()
            .send_p2p_packet(client_id, SendType::Unreliable, payload)
        {
            error!("Error sending packet to {:?}", client_id);
        }

        Ok(())
    }

    fn confirm_connect(&mut self, client_id: SteamId) {
        self.client.networking().accept_p2p_session(client_id);
    }

    fn disconnect(&mut self, client_id: &SteamId, reason: DisconnectionReason) {
        // TODO: do we send the connection error packet?
        let message = Message::Disconnect(reason);
        if let Ok(payload) = bincode::serialize(&message) {
            if let Err(e) = self.send(*client_id, &payload) {
                error!(
                    "Error sending disconnect packet to SteamId {:?}: {}",
                    client_id, e
                );
            }
        }
        self.client.networking().close_p2p_session(*client_id);
    }
    fn is_authenticated(&self, _client_id: SteamId) -> bool {
        // Assume Steam connection is always authenticated
        true
    }

    fn connection_id(&self) -> Self::ConnectionId {
        self.server_id
    }

    fn update(
        &mut self,
        connection_control: &ConnectionControl<SteamId>,
    ) -> (Vec<SteamId>, Vec<(SteamId, DisconnectionReason)>) {
        let mut connecting = vec![];
        let mut disconnected = vec![];

        loop {
            match self.connecting_clients.try_recv() {
                Ok(client_id) => {
                    if connection_control.is_client_permitted(&client_id) {
                        connecting.push(client_id)
                    } else {
                        self.disconnect(&client_id, DisconnectionReason::Denied);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(e) => {
                    // TODO(error): How should we handle when this error occur
                    panic!("Error in the steam connection channel: {}", e);
                }
            }
        }

        loop {
            match self.disconnected_clients.try_recv() {
                Ok(client_id) => {
                    disconnected.push((client_id, DisconnectionReason::TransportError));
                }
                Err(TryRecvError::Empty) => break,
                Err(e) => {
                    // TODO(error): How should we handle when this error occur
                    panic!("Error in the steam connection channel: {}", e);
                }
            }
        }

        (connecting, disconnected)
    }
}
