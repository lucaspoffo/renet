

mod client;
mod server;
mod error;

const NUM_DISCONNECT_PACKETS_TO_SEND: usize = 5;

pub use renetcode::{NetcodeError, NETCODE_KEY_BYTES, NETCODE_USER_DATA_BYTES};

pub use client::{ClientAuthentication, NetcodeClientTransport};
pub use server::{NetcodeServerTransport, ServerConfig, ServerAuthentication};
pub use error::NetcodeTransportError;

pub trait ServerTransport {
    fn send_to(&mut self, client_id: u64, packet: &[u8]);
}

pub trait ClientTransport {
    fn send(&mut self, packet: &[u8]);
}