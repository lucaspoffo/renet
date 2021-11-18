use crate::packet::{Packet, Payload};
use crate::reassembly_fragment::FragmentError;

use bincode::Options;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Error)]
pub enum DisconnectionReason {
    #[error("connection denied")]
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
    ClientAlreadyConnected,
    #[error("reliable channel {channel_id} is out of sync, a message was dropped")]
    ReliableChannelOutOfSync { channel_id: u8 },
}

impl DisconnectionReason {
    pub fn as_packet(&self) -> Result<Payload, RenetError> {
        let packet = Packet::Disconnect(*self);
        let packet = bincode::options().serialize(&packet)?;
        Ok(packet)
    }
}

#[derive(Debug, Error)]
pub enum RenetError {
    #[error("bincode failed to (de)serialize: {0}")]
    BincodeError(#[from] bincode::Error),
    #[error("packet fragmentation error: {0}")]
    FragmentError(#[from] FragmentError),
    #[error("connection error: {0}")]
    ConnectionError(#[from] DisconnectionReason),
}

#[derive(Debug, Error)]
#[error("client not found")]
pub struct ClientNotFound;
