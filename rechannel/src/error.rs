use std::fmt;

/// Possibles reasons for a disconnection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectReason {
    Transport,
    /// Connection was terminated by the server
    DisconnectedByClient,
    /// Connection was terminated by the server
    DisconnectedByServer,
    /// Failed to serialize packet
    PacketSerialization,
    /// Failed to deserialize packet
    PacketDeserialization,
    /// Received message from channel with invalid id
    ReceivedInvalidChannelId(u8),
    /// Error occurred in a send channel
    SendChannelError {
        channel_id: u8,
        error: ChannelError,
    },
    /// Error occurred in a receive channel
    ReceiveChannelError {
        channel_id: u8,
        error: ChannelError,
    },
}

/// Possibles errors that can occur in a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelError {
    /// Reliable channel reached maximum allowed memory
    ReliableChannelMaxMemoryReached,
    /// Received an invalid slice message in the channel.
    InvalidSliceMessage,
}

impl fmt::Display for ChannelError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use ChannelError::*;

        match *self {
            ReliableChannelMaxMemoryReached => write!(fmt, "reliable channel memory usage was exausted"),
            InvalidSliceMessage => write!(fmt, "received an invalid slice packet"),
        }
    }
}

impl fmt::Display for DisconnectReason {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use DisconnectReason::*;

        match *self {
            Transport => write!(fmt, "connection terminated by the transport layer"),
            DisconnectedByClient => write!(fmt, "connection terminated by the client"),
            DisconnectedByServer => write!(fmt, "connection terminated by the server"),
            PacketSerialization => write!(fmt, "failed to serialize packet"),
            PacketDeserialization => write!(fmt, "failed to deserialize packet"),
            ReceivedInvalidChannelId(id) => write!(fmt, "received message with invalid channel {}", id),
            SendChannelError { channel_id, error } => write!(fmt, "send channel {} with error: {}", channel_id, error),
            ReceiveChannelError { channel_id, error } => write!(fmt, "receive channel {} with error: {}", channel_id, error),
        }
    }
}

impl std::error::Error for ChannelError {}

#[derive(Debug)]
pub struct ClientNotFound;

impl std::error::Error for ClientNotFound {}

impl fmt::Display for ClientNotFound {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "client with given id was not found")
    }
}
