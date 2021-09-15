use crate::packet::Payload;
use std::collections::VecDeque;

pub struct UnreliableChannel {
    packet_budget: u64,
    messages_to_send: VecDeque<Payload>,
    messages_received: VecDeque<Payload>,
}

impl UnreliableChannel {
    pub fn new(packet_budget: u64) -> Self {
        Self {
            packet_budget,
            messages_to_send: VecDeque::new(),
            messages_received: VecDeque::new(),
        }
    }

    pub fn process_messages(&mut self, messages: Vec<Payload>) {
        self.messages_received.extend(messages.into_iter());
    }

    pub fn receive_message(&mut self) -> Option<Payload> {
        self.messages_received.pop_front()
    }

    pub fn send_message(&mut self, message_payload: Payload) {
        self.messages_to_send.push_back(message_payload);
    }

    pub fn get_messages_to_send(&mut self, mut available_bytes: u64) -> Vec<Payload> {
        let mut messages = vec![];

        available_bytes = available_bytes.min(self.packet_budget);

        while let Some(message) = self.messages_to_send.pop_front() {
            let message_size = message.len() as u64;
            if message_size > available_bytes {
                break;
            }

            available_bytes -= message_size;
            messages.push(message);
        }

        messages
    }
}
