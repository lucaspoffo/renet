use std::{error::Error, fmt};

mod client;
mod server;

pub use client::*;
pub use server::*;

pub use renetcode::{
    generate_random_bytes, ClientAuthentication, ConnectToken, DisconnectReason as NetcodeDisconnectReason, NetcodeError,
    ServerAuthentication, ServerConfig, TokenGenerationError, NETCODE_KEY_BYTES, NETCODE_USER_DATA_BYTES,
};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Event))]
pub enum NetcodeTransportError {
    Netcode(NetcodeError),
    Renet(crate::DisconnectReason),
    IO(std::io::Error),
}

impl Error for NetcodeTransportError {}

impl fmt::Display for NetcodeTransportError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            NetcodeTransportError::Netcode(ref err) => err.fmt(fmt),
            NetcodeTransportError::Renet(ref err) => err.fmt(fmt),
            NetcodeTransportError::IO(ref err) => err.fmt(fmt),
        }
    }
}

impl From<renetcode::NetcodeError> for NetcodeTransportError {
    fn from(inner: renetcode::NetcodeError) -> Self {
        NetcodeTransportError::Netcode(inner)
    }
}

impl From<renetcode::TokenGenerationError> for NetcodeTransportError {
    fn from(inner: renetcode::TokenGenerationError) -> Self {
        NetcodeTransportError::Netcode(renetcode::NetcodeError::TokenGenerationError(inner))
    }
}

impl From<crate::DisconnectReason> for NetcodeTransportError {
    fn from(inner: crate::DisconnectReason) -> Self {
        NetcodeTransportError::Renet(inner)
    }
}

impl From<std::io::Error> for NetcodeTransportError {
    fn from(inner: std::io::Error) -> Self {
        NetcodeTransportError::IO(inner)
    }
}
