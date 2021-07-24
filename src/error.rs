use crate::reassembly_fragment::FragmentError;

use serde::{Deserialize, Serialize};
use std::{error::Error, io, result};
use thiserror::Error;

pub type Result<T> = result::Result<T, RenetError>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Error)]
pub enum DisconnectionReason {
    #[error("connection denied.")]
    Denied,
    #[error("server has exceeded maximum players capacity")]
    MaxPlayer,
    #[error("connection has timedout")]
    TimedOut,
    #[error("disconnected by server")]
    DisconnectedByServer,
    #[error("disconnected by client")]
    DisconnectedByClient,
    #[error("client with same id already connected")]
    ClientIdAlreadyConnected,
    // TODO: Should the actual error be in this struct?
    #[error("error in channel {channel_id}")]
    ChannelError { channel_id: u8 },
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
    AuthenticationError(Box<dyn Error + Send + Sync + 'static>),
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
