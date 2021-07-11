use crate::reassembly_fragment::FragmentError;

use serde::{Deserialize, Serialize};
use std::{io, result};
use thiserror::Error;

pub type Result<T> = result::Result<T, RenetError>;

#[derive(Clone, Debug, Serialize, Deserialize, Error)]
pub enum DisconnectionReason {
    #[error("connection denied.")]
    Denied,
    #[error("server has exceeded maximum players capacity")]
    MaxPlayer,
    #[error("disconnected by server")]
    DisconnectedByServer,
    #[error("disconnected by client")]
    DisconnectedByClient,
}

#[derive(Debug, Error)]
pub enum RenetError {
    #[error("packet size {got} above the limit, expected < {expected}")]
    MaximumPacketSizeExceeded { expected: usize, got: usize },
    #[error("socket disconnected: {0}")]
    IOError(#[from] io::Error),
    #[error("bincode failed to (de)serialize: {0}")]
    BincodeError(#[from] bincode::Error),
    #[error("error during protocol exchange: {0}")]
    AuthenticationError(Box<dyn std::error::Error>),
    #[error("connection timed out.")]
    ConnectionTimedOut,
    #[error("packet fragmentation error: {0}")]
    FragmentError(#[from] FragmentError),
    #[error("connection error: {0}")]
    ConnectionError(DisconnectionReason),
    #[error("client is disconnected")]
    ClientDisconnected,
    #[error("connection is not established")]
    NotAuthenticated,
    #[error("invalid channel {channel_id}")]
    InvalidChannel { channel_id: u8 },
    #[error("client not found")]
    ClientNotFound,
}
