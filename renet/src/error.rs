use crate::reassembly_fragment::FragmentError;

use serde::{Deserialize, Serialize};

use std::fmt;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum DisconnectionReason {
    /// Server has exceeded maximum players capacity
    MaxConnections,
    TimedOut,
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
            MaxConnections => write!(fmt, "server has reached the limit of connections"),
            TimedOut => write!(fmt, "connection has timed out"),
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
pub enum RenetError {
    MessageSizeAboveLimit,
    ChannelMaxMessagesLimit,
    AlreadySendingBlockMessage,
    ClientDisconnected(DisconnectionReason),
    ClientNotFound,
    FragmentError(FragmentError),
    BincodeError(bincode::Error),
}

impl std::error::Error for RenetError {}

impl fmt::Display for RenetError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use RenetError::*;

        match *self {
            MessageSizeAboveLimit => write!(fmt, "the message is above the limit size"),
            ChannelMaxMessagesLimit => write!(fmt, "the channel has reached the maximum messages"),
            AlreadySendingBlockMessage => {
                write!(fmt, "the connection is already sending a block message")
            }
            ClientNotFound => write!(fmt, "client with given id was not found"),
            ClientDisconnected(reason) => write!(fmt, "client is disconnected: {}", reason),
            BincodeError(ref bincode_err) => write!(fmt, "{}", bincode_err),
            FragmentError(ref fragment_error) => write!(fmt, "{}", fragment_error),
        }
    }
}

impl From<bincode::Error> for RenetError {
    fn from(inner: bincode::Error) -> Self {
        RenetError::BincodeError(inner)
    }
}

impl From<FragmentError> for RenetError {
    fn from(inner: FragmentError) -> Self {
        RenetError::FragmentError(inner)
    }
}
