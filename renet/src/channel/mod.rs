pub(crate) mod block;
pub(crate) mod reliable;
pub(crate) mod unreliable;

use std::time::Duration;

pub use block::BlockChannelConfig;
pub use reliable::ReliableChannelConfig;
pub use unreliable::UnreliableChannelConfig;

use block::BlockChannel;
use reliable::ReliableChannel;
use unreliable::UnreliableChannel;

use crate::error::RenetError;

#[derive(Debug, Clone)]
pub enum ChannelConfig {
    Reliable(ReliableChannelConfig),
    Unreliable(UnreliableChannelConfig),
    Block(BlockChannelConfig),
}

#[derive(Debug)]
pub(crate) enum Channel {
    Reliable(ReliableChannel),
    Unreliable(UnreliableChannel),
    Block(BlockChannel),
}

impl ChannelConfig {
    pub(crate) fn new_channel(&self) -> Channel {
        use ChannelConfig::*;

        match self {
            Reliable(config) => {
                let reliable_channel = ReliableChannel::new(config.clone());
                Channel::Reliable(reliable_channel)
            }
            Unreliable(config) => {
                let unreliable_channel = UnreliableChannel::new(config.clone());
                Channel::Unreliable(unreliable_channel)
            }
            Block(config) => {
                let block_channel = BlockChannel::new(config.clone());
                Channel::Block(block_channel)
            }
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

impl Channel {
    pub fn send_message(&mut self, message: Vec<u8>) -> Result<(), RenetError> {
        match self {
            Channel::Unreliable(channel) => channel.send_message(message),
            Channel::Reliable(channel) => channel.send_message(message),
            Channel::Block(channel) => channel.send_message(message),
        }
    }

    pub fn receive_message(&mut self) -> Option<Vec<u8>> {
        match self {
            Channel::Unreliable(channel) => channel.receive_message(),
            Channel::Reliable(channel) => channel.receive_message(),
            Channel::Block(channel) => channel.receive_message(),
        }
    }

    pub fn process_ack(&mut self, ack: u16) {
        match self {
            Channel::Unreliable(_) => {}
            Channel::Reliable(channel) => channel.process_ack(ack),
            Channel::Block(channel) => channel.process_ack(ack),
        }
    }

    pub fn advance_time(&mut self, duration: Duration) {
        match self {
            Channel::Unreliable(_) => {}
            Channel::Reliable(channel) => channel.advance_time(duration),
            Channel::Block(channel) => channel.advance_time(duration),
        }
    }
}
