use std::net::SocketAddr;

use renet::{NETCODE_KEY_BYTES, NETCODE_USER_DATA_BYTES};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_ID: u64 = 7;

// Helper struct to pass an username in user data inside the ConnectToken
pub struct Username(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterServer {
    pub name: String,
    pub current_clients: u64,
    pub max_clients: u64,
    pub address: SocketAddr,
    pub private_key: [u8; NETCODE_KEY_BYTES],
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestConnection {
    pub username: String,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerUpdate {
    pub current_clients: u64,
    pub max_clients: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyListing {
    pub id: u64,
    pub name: String,
    pub max_clients: u64,
    pub current_clients: u64,
    pub is_protected: bool,
}

impl Username {
    pub fn to_netcode_user_data(&self) -> [u8; NETCODE_USER_DATA_BYTES] {
        let mut user_data = [0u8; NETCODE_USER_DATA_BYTES];
        if self.0.len() > NETCODE_USER_DATA_BYTES - 8 {
            panic!("Username is too big");
        }
        user_data[0..8].copy_from_slice(&(self.0.len() as u64).to_le_bytes());
        user_data[8..self.0.len() + 8].copy_from_slice(self.0.as_bytes());

        user_data
    }

    pub fn from_user_data(user_data: &[u8; NETCODE_USER_DATA_BYTES]) -> Self {
        let mut buffer = [0u8; 8];
        buffer.copy_from_slice(&user_data[0..8]);
        let mut len = u64::from_le_bytes(buffer) as usize;
        len = len.min(NETCODE_USER_DATA_BYTES - 8);
        let data = user_data[8..len + 8].to_vec();
        let username = String::from_utf8(data).unwrap();
        Self(username)
    }
}
