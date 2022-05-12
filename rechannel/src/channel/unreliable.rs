use crate::{
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
};
use std::{collections::VecDeque, time::Duration};

use super::Channel;

/// Configuration for a unreliable and unordered channel.
/// Messages sent in this channel will behave like a udp packet,
/// can be lost and arrive in an different order that they were sent.
#[derive(Debug, Clone)]
pub struct UnreliableChannelConfig {
    /// Channel identifier, unique between all channels
    pub channel_id: u8,
    /// Maximum nuber of bytes that this channel is allowed to write per packet
    pub packet_budget: u64,
    /// Maximum size that a message can have in this channel
    pub max_message_size: u64,
    /// Allowed numbers of messages in the send queue for this channel
    pub message_send_queue_size: usize,
    /// Allowed numbers of messages in the receive queue for this channel
    pub message_receive_queue_size: usize,
}

#[derive(Debug)]
pub(crate) struct UnreliableChannel {
    config: UnreliableChannelConfig,
    messages_to_send: VecDeque<Payload>,
    messages_received: VecDeque<Payload>,
    error: Option<ChannelError>,
}

impl Default for UnreliableChannelConfig {
    fn default() -> Self {
        Self {
            channel_id: 1,
            packet_budget: 2000,
            max_message_size: 1200,
            message_send_queue_size: 256,
            message_receive_queue_size: 256,
        }
    }
}

impl UnreliableChannel {
    pub fn new(config: UnreliableChannelConfig) -> Self {
        Self {
            messages_to_send: VecDeque::with_capacity(config.message_send_queue_size),
            messages_received: VecDeque::with_capacity(config.message_receive_queue_size),
            config,
            error: None,
        }
    }
}

impl Channel for UnreliableChannel {
    fn get_messages_to_send(&mut self, mut available_bytes: u64, _sequence: u16) -> Option<ChannelPacketData> {
        if self.error.is_some() {
            return None;
        }

        let mut messages = vec![];

        available_bytes = available_bytes.min(self.config.packet_budget);

        while let Some(message) = self.messages_to_send.pop_front() {
            let message_size = message.len() as u64;
            if message_size > available_bytes {
                break;
            }

            available_bytes -= message_size;
            messages.push(message);
        }

        if messages.is_empty() {
            return None;
        }

        Some(ChannelPacketData {
            channel_id: self.config.channel_id,
            messages,
        })
    }

    fn advance_time(&mut self, _duration: Duration) {}

    fn process_messages(&mut self, mut messages: Vec<Payload>) {
        if self.error.is_some() {
            return;
        }

        while let Some(message) = messages.pop() {
            if self.messages_received.len() == self.config.message_receive_queue_size {
                log::warn!(
                    "Received message was dropped in unreliable channel {}, reached maximum number of messages {} ",
                    self.config.channel_id,
                    self.config.message_receive_queue_size
                );
                return;
            }
            self.messages_received.push_back(message);
        }
    }

    fn process_ack(&mut self, _ack: u16) {}

    fn send_message(&mut self, payload: Payload) {
        if self.error.is_some() {
            return;
        }

        if self.messages_to_send.len() >= self.config.message_send_queue_size {
            self.error = Some(ChannelError::SendQueueFull);
            log::warn!("Unreliable channel {} has reached the maximum queue size", self.config.channel_id);
            return;
        }

        self.messages_to_send.push_back(payload);
    }

    fn receive_message(&mut self) -> Option<Payload> {
        self.messages_received.pop_front()
    }

    fn can_send_message(&self) -> bool {
        self.messages_to_send.len() < self.config.message_send_queue_size
    }

    fn error(&self) -> Option<ChannelError> {
        self.error
    }
}
