use crate::reassembly_fragment::FragmentError;

use serde::{Deserialize, Serialize};

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisconnectionReason {
    DisconnectedByServer,
    DisconnectedByClient,
    ClientAlreadyConnected,
    InvalidChannelId(u8),
    MismatchingChannelType(u8),
    ReliableChannelOutOfSync(u8),
}

impl fmt::Display for DisconnectionReason {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use DisconnectionReason::*;

        match *self {
            DisconnectedByServer => write!(fmt, "connection terminated by server"),
            DisconnectedByClient => write!(fmt, "connection terminated by client"),
            ClientAlreadyConnected => write!(fmt, "connection with same id alredy exists"),
            InvalidChannelId(id) => write!(fmt, "received message with invalid channel {}", id),
            MismatchingChannelType(id) => write!(fmt, "received message from channel {} with mismatching channel type", id),
            ReliableChannelOutOfSync(id) => write!(fmt, "reliable channel {} is out of sync", id),
        }
    }
}

// Error message not sent
#[derive(Debug)]
pub enum RechannelError {
    MessageSizeAboveLimit,
    ChannelMaxMessagesLimit,
    ClientDisconnected(DisconnectionReason),
    ClientNotFound,
    FragmentError(FragmentError),
    BincodeError(bincode::Error),
}

impl std::error::Error for RechannelError {}

impl fmt::Display for RechannelError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use RechannelError::*;

        match *self {
            MessageSizeAboveLimit => write!(fmt, "the message is above the limit size"),
            ChannelMaxMessagesLimit => write!(fmt, "the channel has reached the maximum messages"),
            ClientNotFound => write!(fmt, "client with given id was not found"),
            ClientDisconnected(reason) => write!(fmt, "client is disconnected: {}", reason),
            BincodeError(ref bincode_err) => write!(fmt, "{}", bincode_err),
            FragmentError(ref fragment_error) => write!(fmt, "{}", fragment_error),
        }
    }
}

impl From<bincode::Error> for RechannelError {
    fn from(inner: bincode::Error) -> Self {
        RechannelError::BincodeError(inner)
    }
}

impl From<FragmentError> for RechannelError {
    fn from(inner: FragmentError) -> Self {
        RechannelError::FragmentError(inner)
    }
}
