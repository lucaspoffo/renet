use crate::{
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
};

use std::{collections::VecDeque, time::Duration};

use bytes::Bytes;

use super::{ReceiveChannel, SendChannel};

/// Configuration for a unreliable and unordered channel.
/// Messages sent in this channel will behave like a udp packet,
/// can be lost and arrive in an different order that they were sent.
#[derive(Debug, Clone)]
pub struct UnreliableChannelConfig {
    /// Channel identifier, unique between all channels
    pub channel_id: u8,
    /// Maximum nuber of bytes that this channel is allowed to write per packet
    pub packet_budget: u64,
    /// Maximum size that a message can have in this channel, for unreliable channel this value
    /// need to be less than the packet budget
    pub max_message_size: u64,
    /// Allowed numbers of messages in the send queue for this channel
    pub message_send_queue_size: usize,
    /// Allowed numbers of messages in the receive queue for this channel
    pub message_receive_queue_size: usize,
}

#[derive(Debug)]
pub(crate) struct SendUnreliableChannel {
    channel_id: u8,
    packet_budget: u64,
    max_message_size: u64,
    message_send_queue_size: usize,
    messages_to_send: VecDeque<Bytes>,
    error: Option<ChannelError>,
}

#[derive(Debug)]
pub(crate) struct ReceiveUnreliableChannel {
    channel_id: u8,
    max_message_size: u64,
    message_receive_queue_size: usize,
    messages_received: VecDeque<Payload>,
    error: Option<ChannelError>,
}

impl Default for UnreliableChannelConfig {
    fn default() -> Self {
        Self {
            channel_id: 1,
            packet_budget: 6000,
            max_message_size: 3000,
            message_send_queue_size: 256,
            message_receive_queue_size: 256,
        }
    }
}

impl SendUnreliableChannel {
    pub fn new(config: UnreliableChannelConfig) -> Self {
        assert!(config.max_message_size <= config.packet_budget);

        Self {
            channel_id: config.channel_id,
            packet_budget: config.packet_budget,
            max_message_size: config.max_message_size,
            message_send_queue_size: config.message_send_queue_size,
            messages_to_send: VecDeque::with_capacity(config.message_send_queue_size),
            error: None,
        }
    }
}

impl SendChannel for SendUnreliableChannel {
    fn get_messages_to_send(&mut self, mut available_bytes: u64, _sequence: u16) -> Option<ChannelPacketData> {
        if self.error.is_some() {
            return None;
        }

        let mut messages = vec![];
        available_bytes = available_bytes.min(self.packet_budget);

        while let Some(message) = self.messages_to_send.pop_front() {
            let message_size = message.len() as u64;
            if message_size > available_bytes {
                continue;
            }

            available_bytes -= message_size;
            messages.push(message.to_vec());
        }

        if messages.is_empty() {
            return None;
        }

        Some(ChannelPacketData {
            channel_id: self.channel_id,
            messages,
        })
    }

    fn process_ack(&mut self, _ack: u16) {}

    fn advance_time(&mut self, _duration: Duration) {}

    fn send_message(&mut self, payload: Bytes) {
        if self.error.is_some() {
            return;
        }

        if payload.len() as u64 > self.max_message_size {
            log::error!(
                "Tried to send unreliable message with size above the limit, got {} bytes, expected less than {}",
                payload.len(),
                self.max_message_size
            );
            self.error = Some(ChannelError::SentMessageAboveMaxSize);
            return;
        }

        if self.messages_to_send.len() >= self.message_send_queue_size {
            self.error = Some(ChannelError::SendQueueFull);
            log::warn!("Unreliable channel {} has reached the maximum queue size", self.channel_id);
            return;
        }

        self.messages_to_send.push_back(payload);
    }

    fn can_send_message(&self) -> bool {
        self.messages_to_send.len() < self.message_send_queue_size
    }

    fn error(&self) -> Option<ChannelError> {
        self.error
    }
}

impl ReceiveUnreliableChannel {
    pub fn new(config: UnreliableChannelConfig) -> Self {
        assert!(config.max_message_size <= config.packet_budget);

        Self {
            channel_id: config.channel_id,
            max_message_size: config.max_message_size,
            message_receive_queue_size: config.message_receive_queue_size,
            messages_received: VecDeque::with_capacity(config.message_receive_queue_size),
            error: None,
        }
    }
}

impl ReceiveChannel for ReceiveUnreliableChannel {
    fn process_messages(&mut self, mut messages: Vec<Payload>) {
        if self.error.is_some() {
            return;
        }

        while let Some(message) = messages.pop() {
            if message.len() as u64 > self.max_message_size {
                log::error!(
                    "Received unreliable message with size above the limit, got {} bytes, expected less than {}",
                    message.len(),
                    self.max_message_size
                );
                self.error = Some(ChannelError::ReceivedMessageAboveMaxSize);
                return;
            }

            if self.messages_received.len() == self.message_receive_queue_size {
                log::warn!(
                    "Received message was dropped in unreliable channel {}, reached maximum number of messages {}",
                    self.channel_id,
                    self.message_receive_queue_size
                );
                return;
            }

            self.messages_received.push_back(message);
        }
    }

    fn receive_message(&mut self) -> Option<Payload> {
        self.messages_received.pop_front()
    }

    fn error(&self) -> Option<ChannelError> {
        self.error
    }
}
