pub(crate) mod block;
pub(crate) mod reliable;
pub(crate) mod unreliable;

use std::time::Duration;

pub use block::BlockChannelConfig;
pub use reliable::ReliableChannelConfig;
pub use unreliable::UnreliableChannelConfig;

use bytes::Bytes;

use crate::{
    channel::{
        block::{ReceiveBlockChannel, SendBlockChannel},
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
    Block(BlockChannelConfig),
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
            Block(config) => (
                Box::new(SendBlockChannel::new(config.clone())),
                Box::new(ReceiveBlockChannel::new(config.clone())),
            ),
        }
    }

    pub fn channel_id(&self) -> u8 {
        match self {
            ChannelConfig::Unreliable(config) => config.channel_id,
            ChannelConfig::Reliable(config) => config.channel_id,
            ChannelConfig::Block(config) => config.channel_id,
        }
    }
}

pub(crate) trait SendChannel: std::fmt::Debug {
    fn get_messages_to_send(&mut self, available_bytes: u64, sequence: u16) -> Option<ChannelPacketData>;
    fn process_ack(&mut self, ack: u16);
    // TODO: maybe the timer should check with current_time to see if is completed instead of advancing it by the delta every tick
    // So we would pass the current_time in the get_messages_to_send
    fn advance_time(&mut self, duration: Duration);
    fn send_message(&mut self, payload: Bytes);
    fn can_send_message(&self) -> bool;
    fn error(&self) -> Option<ChannelError>;
}

pub(crate) trait ReceiveChannel: std::fmt::Debug {
    fn process_messages(&mut self, messages: Vec<Payload>);
    fn receive_message(&mut self) -> Option<Payload>;
    fn error(&self) -> Option<ChannelError>;
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

impl From<BlockChannelConfig> for ChannelConfig {
    fn from(config: BlockChannelConfig) -> Self {
        Self::Block(config)
    }
}
