use crate::{
    error::RechannelError,
    packet::{Payload, UnreliableChannelData},
};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct UnreliableChannelConfig {
    pub channel_id: u8,
    pub packet_budget: u64,
    pub max_message_size: u64,
    pub message_send_queue_size: usize,
    pub message_receive_queue_size: usize,
}

#[derive(Debug)]
pub(crate) struct UnreliableChannel {
    config: UnreliableChannelConfig,
    messages_to_send: VecDeque<Payload>,
    messages_received: VecDeque<Payload>,
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
        }
    }

    pub fn process_messages(&mut self, mut messages: Vec<Payload>) {
        while let Some(message) = messages.pop() {
            if self.messages_received.len() == self.config.message_receive_queue_size {
                // TODO(warn) exceeded limit of unreliable messages
                return;
            }
            self.messages_received.push_back(message);
        }
    }

    pub fn receive_message(&mut self) -> Option<Payload> {
        self.messages_received.pop_front()
    }

    pub fn send_message(&mut self, message_payload: Payload) -> Result<(), RechannelError> {
        if self.messages_to_send.len() > self.config.message_send_queue_size {
            return Err(RechannelError::ChannelMaxMessagesLimit);
        }
        self.messages_to_send.push_back(message_payload);
        Ok(())
    }

    pub fn get_messages_to_send(&mut self, mut available_bytes: u64) -> Option<UnreliableChannelData> {
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

        Some(UnreliableChannelData { channel_id: self.config.channel_id, messages })
    }
}
