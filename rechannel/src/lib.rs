pub mod channels;
pub mod error;
mod packet;
pub mod remote_connection;
pub mod server;

pub use bytes::Bytes;

// TODO: rtt
// TODO: packet loss
// TODO: bandwidth ( circle buffer SentInfo per tick)
// TODO: channel priority: Vec<ChannelPriority>
// enum ChannelPriority { Reliable(index: u8), Unreliable(index: u8) }