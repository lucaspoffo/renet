pub(crate) mod block;
pub(crate) mod reliable;
pub(crate) mod unreliable;

use std::time::Duration;

pub use block::ChunkChannelConfig;
pub use reliable::ReliableChannelConfig;
pub use unreliable::UnreliableChannelConfig;

use bytes::Bytes;

use crate::{
    channel::{
        block::{ReceiveChunkChannel, SendChunkChannel},
        reliable::{ReceiveReliableChannel, SendReliableChannel},
        unreliable::{ReceiveUnreliableChannel, SendUnreliableChannel},
    },
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
};

/// Configuration for the different types of channels.
#[derive(Debug, Clone)]
pub enum ChannelConfig {
    Reliable(ReliableChannelConfig),
    Unreliable(UnreliableChannelConfig),
    Chunk(ChunkChannelConfig),
}

pub(crate) trait SendChannel: std::fmt::Debug {
    fn get_messages_to_send(&mut self, available_bytes: u64, sequence: u16, current_time: Duration) -> Option<ChannelPacketData>;
    fn send_message(&mut self, payload: Bytes, current_time: Duration);
    fn process_ack(&mut self, ack: u16);
    fn can_send_message(&self) -> bool;
    fn error(&self) -> Option<ChannelError>;
}

pub(crate) trait ReceiveChannel: std::fmt::Debug {
    fn process_messages(&mut self, messages: Vec<Payload>);
    fn receive_message(&mut self) -> Option<Payload>;
    fn error(&self) -> Option<ChannelError>;
}

/// Default channels used when using the default configuration.
/// Use this enum only when using the default channels configuration.
pub enum DefaultChannel {
    Reliable,
    Unreliable,
    Chunk,
}

impl From<ReliableChannelConfig> for ChannelConfig {
    fn from(config: ReliableChannelConfig) -> Self {
        Self::Reliable(config)
    }
}

impl From<UnreliableChannelConfig> for ChannelConfig {
    fn from(config: UnreliableChannelConfig) -> Self {
        Self::Unreliable(config)
    }
}

impl From<ChunkChannelConfig> for ChannelConfig {
    fn from(config: ChunkChannelConfig) -> Self {
        Self::Chunk(config)
    }
}

impl ChannelConfig {
    pub(crate) fn new_channels(
        &self,
    ) -> (
        Box<dyn SendChannel + Send + Sync + 'static>,
        Box<dyn ReceiveChannel + Send + Sync + 'static>,
    ) {
        use ChannelConfig::*;

        match self {
            Unreliable(config) => (
                Box::new(SendUnreliableChannel::new(config.clone())),
                Box::new(ReceiveUnreliableChannel::new(config.clone())),
            ),
            Reliable(config) => (
                Box::new(SendReliableChannel::new(config.clone())),
                Box::new(ReceiveReliableChannel::new(config.clone())),
            ),
            Chunk(config) => (
                Box::new(SendChunkChannel::new(config.clone())),
                Box::new(ReceiveChunkChannel::new(config.clone())),
            ),
        }
    }

    pub fn channel_id(&self) -> u8 {
        match self {
            ChannelConfig::Unreliable(config) => config.channel_id,
            ChannelConfig::Reliable(config) => config.channel_id,
            ChannelConfig::Chunk(config) => config.channel_id,
        }
    }
}

impl From<DefaultChannel> for u8 {
    fn from(channel: DefaultChannel) -> Self {
        match channel {
            DefaultChannel::Reliable => 0,
            DefaultChannel::Unreliable => 1,
            DefaultChannel::Chunk => 2,
        }
    }
}

impl DefaultChannel {
    pub fn config() -> Vec<ChannelConfig> {
        vec![
            ChannelConfig::Reliable(Default::default()),
            ChannelConfig::Unreliable(Default::default()),
            ChannelConfig::Chunk(Default::default()),
        ]
    }
}
