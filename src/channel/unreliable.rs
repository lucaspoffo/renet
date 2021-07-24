use crate::{
    channel::{Channel, ChannelConfig},
    packet::Payload,
};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct UnreliableUnorderedChannelConfig {
    pub max_message_per_packet: u32,
    pub packet_budget: Option<u32>,
}

impl Default for UnreliableUnorderedChannelConfig {
    fn default() -> Self {
        Self {
            max_message_per_packet: 256,
            packet_budget: None,
        }
    }
}

pub struct UnreliableUnorderedChannel {
    config: UnreliableUnorderedChannelConfig,
    messages_to_send: VecDeque<Payload>,
    messages_received: VecDeque<Payload>,
}

impl ChannelConfig for UnreliableUnorderedChannelConfig {
    fn new_channel(&self) -> Box<dyn Channel> {
        Box::new(UnreliableUnorderedChannel::new(self.clone()))
    }
}

impl UnreliableUnorderedChannel {
    fn new(config: UnreliableUnorderedChannelConfig) -> Self {
        Self {
            messages_to_send: VecDeque::new(),
            messages_received: VecDeque::new(),
            config,
        }
    }
}

impl Channel for UnreliableUnorderedChannel {
    fn process_messages(&mut self, messages: Vec<Payload>) {
        self.messages_received.extend(messages.into_iter());
    }

    fn receive_message(&mut self) -> Option<Payload> {
        self.messages_received.pop_front()
    }

    fn send_message(&mut self, message_payload: Payload) {
        self.messages_to_send.push_back(message_payload);
    }

    fn get_messages_to_send(&mut self, mut available_bytes: u32, _sequence: u16) -> Vec<Payload> {
        let mut num_messages = 0;
        let mut messages = vec![];

        if let Some(packet_budget) = self.config.packet_budget {
            available_bytes = available_bytes.min(packet_budget);
        }

        while let Some(message) = self.messages_to_send.pop_front() {
            if num_messages == self.config.max_message_per_packet {
                break;
            }

            let message_size = message.len() as u32;
            if message_size > available_bytes {
                break;
            }

            available_bytes -= message_size;
            num_messages += 1;
            messages.push(message);
        }

        messages
    }

    fn error(&self) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        None
    }

    // Since this is an unreliable channel, we do nothing with the ack.
    fn process_ack(&mut self, _ack: u16) {}
}
