pub(crate) mod block;
pub(crate) mod reliable;
pub(crate) mod unreliable;

use std::time::Duration;

pub use block::BlockChannelConfig;
pub use reliable::ReliableChannelConfig;
pub use unreliable::UnreliableChannelConfig;

use bytes::Bytes;

use crate::{
    channel::{block::BlockChannel, reliable::ReliableChannel, unreliable::UnreliableChannel},
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
    pub(crate) fn new_channel(&self) -> Box<dyn Channel + Send + Sync + 'static> {
        use ChannelConfig::*;

        match self {
            Reliable(config) => Box::new(ReliableChannel::new(config.clone())),
            Unreliable(config) => Box::new(UnreliableChannel::new(config.clone())),
            Block(config) => Box::new(BlockChannel::new(config.clone())),
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

pub trait Channel: std::fmt::Debug {
    fn get_messages_to_send(&mut self, available_bytes: u64, sequence: u16) -> Option<ChannelPacketData>;
    fn advance_time(&mut self, duration: Duration);
    fn process_messages(&mut self, messages: Vec<Payload>);
    fn process_ack(&mut self, ack: u16);
    fn send_message(&mut self, payload: Bytes);
    fn receive_message(&mut self) -> Option<Payload>;
    fn can_send_message(&self) -> bool;
    fn error(&self) -> Option<ChannelError>;
}
