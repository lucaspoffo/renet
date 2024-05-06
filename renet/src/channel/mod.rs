pub(crate) mod reliable;
pub(crate) mod slice_constructor;
pub(crate) mod unreliable;

use std::time::Duration;

pub(crate) use slice_constructor::SliceConstructor;

/// Delivery guarantee of a channel
#[derive(Debug, Clone)]
pub enum SendType {
    // Messages can be lost or received out of order.
    Unreliable,
    /// Messages are guaranteed to be received and in the same order they were sent.
    ReliableOrdered {
        resend_time: Duration,
    },
    /// Messages are guaranteed to be received but may be in an different order that they were sent.
    ReliableUnordered {
        resend_time: Duration,
    },
}

/// Configuration of a channel for a server or client
/// Channels are unilateral and message based.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Channel identifier, must be unique within its own list,
    /// but it can be repeated between the server and client lists.
    pub channel_id: u8,
    /// Maximum number of bytes that the channel may hold without acknowledgement of messages before becoming full.
    /// Unreliable channels will drop new messages when this value is reached.
    /// Reliable channels will cause a disconnect when this value is reached.
    pub max_memory_usage_bytes: usize,
    /// Delivery guarantee of the channel.
    pub send_type: SendType,
}

/// Utility enumerator when using the default channels configuration.
/// The default configuration has 3 channels: unreliable, reliable ordered, and reliable unordered.
pub enum DefaultChannel {
    Unreliable,
    ReliableOrdered,
    ReliableUnordered,
}

impl From<DefaultChannel> for u8 {
    fn from(channel: DefaultChannel) -> Self {
        match channel {
            DefaultChannel::Unreliable => 0,
            DefaultChannel::ReliableUnordered => 1,
            DefaultChannel::ReliableOrdered => 2,
        }
    }
}

impl DefaultChannel {
    pub fn config() -> Vec<ChannelConfig> {
        vec![
            ChannelConfig {
                channel_id: 0,
                max_memory_usage_bytes: 5 * 1024 * 1024,
                send_type: SendType::Unreliable,
            },
            ChannelConfig {
                channel_id: 1,
                max_memory_usage_bytes: 5 * 1024 * 1024,
                send_type: SendType::ReliableUnordered {
                    resend_time: Duration::from_millis(300),
                },
            },
            ChannelConfig {
                channel_id: 2,
                max_memory_usage_bytes: 5 * 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::from_millis(300),
                },
            },
        ]
    }
}
