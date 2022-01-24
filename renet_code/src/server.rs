use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{packet::ConnectionRequest, token::PrivateConnectToken, NETCODE_KEY_BYTES, NETCODE_VERSION_INFO};

struct Client;

struct Server {
    clients: HashMap<u64, Client>,
    protocol_id: u64,
    connect_key: [u8; NETCODE_KEY_BYTES],
    max_clients: usize,
    challenge_sequence: u64,
    challenge_key: [u8; NETCODE_KEY_BYTES],
    address: SocketAddr,
}

impl Server {
    pub fn handle_connection_request(&mut self, mut request: ConnectionRequest) {
        if let Some(connect_token) = self.validate_client_token(&mut request) {
            if self.clients.contains_key(&connect_token.client_id) {
                // TODO: handle when client already connected
                // Resend client challenge packet packet
                return;
            }

            if self.clients.len() >= self.max_clients {
                // TODO: handle max client error
                // send denied packet
                return;
            }

            let client = self.clients.entry(connect_token.client_id).or_insert_with(|| Client);
        } else {
            // TODO(error): handle when failed to verify client token
        }
    }

    // TODO(error): should we return Result?
    pub fn validate_client_token(&self, request: &mut ConnectionRequest) -> Option<PrivateConnectToken> {
        if request.version_info != *NETCODE_VERSION_INFO {
            // TODO(error)
            return None;
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if now > request.expire_timestamp {
            // TODO(error)
            return None;
        }

        let token = PrivateConnectToken::decode(
            &mut request.data,
            self.protocol_id,
            request.expire_timestamp,
            request.sequence,
            &self.connect_key,
        )
        .ok()?;

        let in_host_list = token.server_addresses.iter().any(|host| *host == Some(self.address));
        if in_host_list {
            // TODO(error)
            return Some(token);
        }

        None
    }
}
