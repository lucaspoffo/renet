use crate::packet::{Packet, Payload};
use crate::reassembly_fragment::FragmentError;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use bincode::Options;

use std::{error::Error, io};


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
    #[error("crossbeam channel error while sending or receiving a message locally")]
    CrossbeamChannelError,
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

// TODO: Can this be separated into Server/Client Error or Server/Connection Error
#[derive(Debug, Error)]
pub enum RenetError {
    #[error("socket disconnected: {0}")]
    IOError(#[from] io::Error),
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
