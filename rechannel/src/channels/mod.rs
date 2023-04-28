pub(crate) mod reliable;
pub(crate) mod slice_constructor;
pub(crate) mod unreliable;

use std::time::Duration;

pub(crate) use slice_constructor::SliceConstructor;

pub(crate) const SLICE_SIZE: usize = 1200;

#[derive(Debug, Clone)]
pub enum SendType {
    Unreliable,
    ReliableOrdered { resend_time: Duration },
    ReliableUnordered { resend_time: Duration },
}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Channel identifier, unique between all channels
    pub channel_id: u8,
    /// Maximum nuber of bytes that this channel is allowed to write per packet
    // pub packet_budget: u64,
    /// Maximum number of bytes that the channel may hold
    /// Unreliable channels will drop new messages when this value is reached
    /// Reliable channels will cause a disconnect when this value is reached
    pub max_memory_usage_bytes: usize,
    pub send_type: SendType,
}

/// Default channels used when using the default configuration.
/// Use this enum only when using the default channels configuration.
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
