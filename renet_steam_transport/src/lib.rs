use std::sync::mpsc::{self, Receiver};

use steamworks::{CallbackHandle, ClientManager, Client, P2PSessionRequest, SteamId, networking_types::NetworkingIdentity, networking_sockets::NetConnection};

pub struct SteamClientTransport {
    net_connection: NetConnection<ClientManager>
}

impl SteamClientTransport {
    pub fn new(server_id: NetworkingIdentity, client: &Client<ClientManager>) -> Self {
        let networking_sockets = client.networking_sockets();
        let net_connection = networking_sockets.connect_p2p(server_id, 0, None).unwrap();

        Self {
            net_connection
        }
    }

    pub fn is_connected(&self) {
        self.net_connection
    }
}
