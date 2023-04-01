pub mod reliable;
mod slice_constructor;
pub mod unreliable;

use std::time::Duration;

pub use slice_constructor::SliceConstructor;

pub const SLICE_SIZE: usize = 1200;

pub enum SendType {
    Unreliable,
    ReliableOrdered { resend_time: Duration },
    ReliableUnordered { resend_time: Duration },
}

pub struct ChannelConfig {
    /// Channel identifier, unique between all channels
    pub channel_id: u8,
    /// Maximum nuber of bytes that this channel is allowed to write per packet
    pub packet_budget: u64,
    /// Maximum number of bytes that the channel may hold
    /// Unreliable channels will drop new messages when this value is reached
    /// Reliable channels will cause a disconnect when this value is reached
    pub max_memory_usage_bytes: usize,
    pub send_type: SendType,
}
