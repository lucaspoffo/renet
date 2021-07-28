use crate::reassembly_fragment::FragmentError;

use serde::{Deserialize, Serialize};
use std::{error::Error, io};
use thiserror::Error;

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

// TODO: Can this be separated into Server/Client Error or Server/Connection Error
#[derive(Debug, Error)]
pub enum RenetError {
    #[error("socket disconnected: {0}")]
    IOError(#[from] io::Error),
    #[error("bincode failed to (de)serialize: {0}")]
    BincodeError(#[from] bincode::Error),
    #[error("error during protocol exchange: {0}")]
    AuthenticationError(Box<dyn Error + Send + Sync + 'static>),
    #[error("packet fragmentation error: {0}")]
    FragmentError(#[from] FragmentError),
    #[error("connection error: {0}")]
    ConnectionError(DisconnectionReason),
    #[error("client is disconnected")]
    ClientDisconnected,
}

#[derive(Debug, Error)]
pub enum MessageError {
    #[error("invalid channel {channel_id}")]
    InvalidChannel { channel_id: u8 },
    #[error("client not found")]
    ClientNotFound,
}
