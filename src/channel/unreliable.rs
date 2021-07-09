use crate::channel::{Channel, ChannelConfig, Message};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct UnreliableUnorderedChannelConfig {
    packet_budget_bytes: Option<u32>,
    max_message_per_packet: u32,
}

impl Default for UnreliableUnorderedChannelConfig {
    fn default() -> Self {
        Self {
            packet_budget_bytes: None,
            max_message_per_packet: 256,
        }
    }
}

pub struct UnreliableUnorderedChannel {
    send_message_id: u16,
    config: UnreliableUnorderedChannelConfig,
    messages_to_send: VecDeque<Message>,
    messages_received: VecDeque<Message>,
}

impl ChannelConfig for UnreliableUnorderedChannelConfig {
    fn new_channel(&self) -> Box<dyn Channel> {
        Box::new(UnreliableUnorderedChannel::new(self.clone()))
    }
}

impl UnreliableUnorderedChannel {
    fn new(config: UnreliableUnorderedChannelConfig) -> Self {
        Self {
            send_message_id: 0,
            messages_to_send: VecDeque::new(),
            messages_received: VecDeque::new(),
            config,
        }
    }
}

impl Channel for UnreliableUnorderedChannel {
    fn process_messages(&mut self, messages: Vec<Message>) {
        self.messages_received.extend(messages.into_iter());
    }

    fn receive_message(&mut self) -> Option<Box<[u8]>> {
        if let Some(message) = self.messages_received.pop_front() {
            Some(message.payload)
        } else {
            None
        }
    }

    fn send_message(&mut self, message_payload: Box<[u8]>) {
        let message = Message::new(self.send_message_id, message_payload);
        self.send_message_id = self.send_message_id.wrapping_add(1);
        self.messages_to_send.push_back(message);
    }

    fn get_messages_to_send(
        &mut self,
        available_bits: Option<u32>,
        _sequence: u16,
    ) -> Option<Vec<Message>> {
        // TODO: Should we even be doing this?
        let available_bits = available_bits.unwrap_or(u32::MAX);

        let mut available_bits = if let Some(packet_budget) = self.config.packet_budget_bytes {
            std::cmp::min(packet_budget * 8, available_bits)
        } else {
            available_bits
        };

        let mut num_messages = 0;
        let mut messages = vec![];

        while let Some(message) = self.messages_to_send.pop_front() {
            if num_messages == self.config.max_message_per_packet {
                break;
            }

            // TODO: Magic number to give up trying to fit a message.
            if available_bits < 40 {
                break;
            }

            let message_size = message.serialized_size_bytes() * 8;
            // TODO: if there is not enough space we simply drop the message
            // Should we do something else?
            if message_size <= available_bits {
                available_bits -= message_size;
                num_messages += 1;
                messages.push(message);
            }
        }

        if !messages.is_empty() {
            Some(messages)
        } else {
            None
        }
    }

    // Since this is an unreliable channel
    // we do nothing with the ack.
    fn process_ack(&mut self, _ack: u16) {}
}
