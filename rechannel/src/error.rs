use crate::reassembly_fragment::FragmentError;

use serde::{Deserialize, Serialize};

use std::fmt;

/// Possibles reasons for a disconnection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisconnectionReason {
    /// Connection terminated by server
    DisconnectedByServer,
    /// Connection terminated by client
    DisconnectedByClient,
    /// Channel with given Id was not found
    InvalidChannelId(u8),
    /// Error occurred in a send channel
    SendChannelError { channel_id: u8, error: ChannelError },
    /// Error occurred in a receive channel
    ReceiveChannelError { channel_id: u8, error: ChannelError },
}

/// Possibles errors that can occur in a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelError {
    /// The reliable channel with given Id is out of sync
    ReliableChannelOutOfSync,
    /// The channel send queue has reach it's maximum
    SendQueueFull,
    /// Error occurred during (de)serialization
    // TODO: rename to SerializationFailure
    FailedToSerialize,
    /// Tried to send a message that is above the channel max message size.
    SentMessageAboveMaxSize,
    /// Tried to send an empty message
    SentEmptyMessage,
    /// Received a message above above the channel max message size.
    ReceivedMessageAboveMaxSize,
    /// Received an invalid slice message in a block channel.
    InvalidSliceMessage,
}

impl fmt::Display for ChannelError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use ChannelError::*;

        match *self {
            ReliableChannelOutOfSync => write!(fmt, "reliable channel out of sync"),
            SendQueueFull => write!(fmt, "send queue was full"),
            FailedToSerialize => write!(fmt, "failed to serialize or deserialize"),
            SentMessageAboveMaxSize => write!(fmt, "sent message above the channel max message size"),
            SentEmptyMessage => write!(fmt, "sent empty message"),
            ReceivedMessageAboveMaxSize => write!(fmt, "received message above the channel max message size"),
            InvalidSliceMessage => write!(fmt, "received an invalid slice message in a block channel"),
        }
    }
}

impl fmt::Display for DisconnectionReason {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use DisconnectionReason::*;

        match *self {
            DisconnectedByServer => write!(fmt, "connection terminated by server"),
            DisconnectedByClient => write!(fmt, "connection terminated by client"),
            InvalidChannelId(id) => write!(fmt, "received message with invalid channel {}", id),
            SendChannelError { channel_id, error } => write!(fmt, "send channel {} with error: {}", channel_id, error),
            ReceiveChannelError { channel_id, error } => write!(fmt, "receive channel {} with error: {}", channel_id, error),
        }
    }
}

impl std::error::Error for ChannelError {}

#[derive(Debug)]
pub enum RechannelError {
    /// The channel has reached the maximum messages capacity defined in the channel configuration
    ChannelMaxMessagesLimit,
    ClientDisconnected(DisconnectionReason),
    ClientNotFound,
    /// An error occurred when processing a fragmented packet
    FragmentError(FragmentError),
    BincodeError(bincode::Error),
}

impl std::error::Error for RechannelError {}

impl fmt::Display for RechannelError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use RechannelError::*;

        match *self {
            ChannelMaxMessagesLimit => write!(fmt, "the channel has reached the maximum messages capacity"),
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
