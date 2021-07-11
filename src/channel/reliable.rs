use crate::channel::{Channel, ChannelConfig};
use crate::packet::Payload;
use crate::sequence_buffer::SequenceBuffer;

use log::error;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReliableMessage {
    id: u16,
    payload: Payload,
}

impl ReliableMessage {
    pub fn new(id: u16, payload: Payload) -> Self {
        Self { id, payload }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReliableMessageSent {
    reliable_message: ReliableMessage,
    last_time_sent: Option<Instant>,
}

impl ReliableMessageSent {
    pub fn new(reliable_message: ReliableMessage) -> Self {
        Self {
            reliable_message,
            last_time_sent: None,
        }
    }
}

#[derive(Debug, Clone)]
struct PacketSent {
    acked: bool,
    time_sent: Instant,
    messages_id: Vec<u16>,
}

impl PacketSent {
    pub fn new(messages_id: Vec<u16>) -> Self {
        Self {
            acked: false,
            time_sent: Instant::now(),
            messages_id,
        }
    }
}

impl Default for PacketSent {
    fn default() -> Self {
        Self {
            acked: false,
            time_sent: Instant::now(),
            messages_id: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReliableOrderedChannelConfig {
    pub sent_packet_buffer_size: usize,
    pub message_send_queue_size: usize,
    pub message_receive_queue_size: usize,
    pub max_message_per_packet: u32,
    pub message_resend_time: Duration,
}

impl Default for ReliableOrderedChannelConfig {
    fn default() -> Self {
        Self {
            sent_packet_buffer_size: 1024,
            message_send_queue_size: 1024,
            message_receive_queue_size: 1024,
            max_message_per_packet: 256,
            message_resend_time: Duration::from_millis(100),
        }
    }
}

impl ChannelConfig for ReliableOrderedChannelConfig {
    fn new_channel(&self) -> Box<dyn Channel> {
        Box::new(ReliableOrderedChannel::new(self.clone()))
    }
}

pub struct ReliableOrderedChannel {
    config: ReliableOrderedChannelConfig,
    packets_sent: SequenceBuffer<PacketSent>,
    messages_send: SequenceBuffer<ReliableMessageSent>,
    messages_received: SequenceBuffer<ReliableMessage>,
    send_message_id: u16,
    received_message_id: u16,
    num_messages_sent: u64,
    num_messages_received: u64,
    oldest_unacked_message_id: u16,
}

impl ReliableOrderedChannel {
    pub fn new(config: ReliableOrderedChannelConfig) -> Self {
        Self {
            packets_sent: SequenceBuffer::with_capacity(config.sent_packet_buffer_size),
            messages_send: SequenceBuffer::with_capacity(config.message_send_queue_size),
            messages_received: SequenceBuffer::with_capacity(config.message_receive_queue_size),
            send_message_id: 0,
            received_message_id: 0,
            num_messages_received: 0,
            num_messages_sent: 0,
            oldest_unacked_message_id: 0,
            config,
        }
    }

    pub fn has_messages_to_send(&self) -> bool {
        self.oldest_unacked_message_id != self.send_message_id
    }

    fn update_oldest_message_ack(&mut self) {
        let stop_id = self.messages_send.sequence();

        while self.oldest_unacked_message_id != stop_id
            && !self.messages_send.exists(self.oldest_unacked_message_id)
        {
            self.oldest_unacked_message_id = self.oldest_unacked_message_id.wrapping_add(1);
        }
    }
}

impl Channel for ReliableOrderedChannel {
    fn get_messages_to_send(
        &mut self,
        mut available_bytes: u32,
        sequence: u16,
    ) -> Option<Vec<Payload>> {
        if !self.has_messages_to_send() {
            return None;
        }

        let message_limit = self
            .config
            .message_send_queue_size
            .min(self.config.message_receive_queue_size);

        let mut num_messages = 0;
        let mut messages = vec![];
        let now = Instant::now();
        for i in 0..message_limit {
            if num_messages == self.config.max_message_per_packet {
                break;
            }
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);
            let message_send = self.messages_send.get_mut(message_id);
            if let Some(message_send) = message_send {
                let send = match message_send.last_time_sent {
                    Some(last_time_sent) => {
                        (last_time_sent + self.config.message_resend_time) <= now
                    }
                    None => true,
                };

                let serialized_size = match bincode::serialized_size(&message_send.reliable_message)
                {
                    Ok(size) => size as u32,
                    Err(e) => {
                        error!("Failed to get reliable message size: {}", e);
                        continue;
                    }
                };

                if send && serialized_size <= available_bytes {
                    num_messages += 1;
                    available_bytes -= serialized_size;
                    message_send.last_time_sent = Some(now);

                    messages.push(message_send.reliable_message.clone());
                }
            }
        }

        if !messages.is_empty() {
            let mut messages_id = vec![];
            let mut payloads = vec![];
            for message in messages.iter() {
                match bincode::serialize(message) {
                    Ok(p) => {
                        messages_id.push(message.id);
                        payloads.push(p);
                    }
                    Err(e) => {
                        error!("Failed to serialize reliable message: {}", e);
                    }
                }
            }

            let packet_sent = PacketSent::new(messages_id);
            self.packets_sent.insert(sequence, packet_sent);
            return Some(payloads);
        }
        None
    }

    fn process_messages(&mut self, messages: Vec<Payload>) {
        for message in messages.iter() {
            match bincode::deserialize::<ReliableMessage>(message) {
                Ok(message) => {
                    // TODO: validate min max message_id based on config queue size
                    if !self.messages_received.exists(message.id) {
                        self.messages_received.insert(message.id, message);
                    }
                }
                Err(e) => error!("Failed to deserialize reliable message: {}", e),
            }
        }
    }

    fn process_ack(&mut self, ack: u16) {
        if let Some(sent_packet) = self.packets_sent.get_mut(ack) {
            if sent_packet.acked {
                return;
            }
            sent_packet.acked = true;

            for &message_id in sent_packet.messages_id.iter() {
                if self.messages_send.exists(message_id) {
                    self.messages_send.remove(message_id);
                }
            }
            self.update_oldest_message_ack();
        }
    }

    fn send_message(
        &mut self,
        message_payload: Payload,
    ) -> Result<(), Box<dyn Error + Sync + Send + 'static>> {
        let message_id = self.send_message_id;
        self.send_message_id = self.send_message_id.wrapping_add(1);

        let entry = ReliableMessageSent::new(ReliableMessage::new(message_id, message_payload));
        self.messages_send.insert(message_id, entry);

        self.num_messages_sent += 1;

        // TODO: Should emit error when full
        Ok(())
    }

    fn receive_message(&mut self) -> Option<Payload> {
        let received_message_id = self.received_message_id;

        if !self.messages_received.exists(received_message_id) {
            return None;
        }

        self.received_message_id = self.received_message_id.wrapping_add(1);
        self.num_messages_received += 1;

        self.messages_received
            .remove(received_message_id)
            .map(|m| m.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
    enum TestMessages {
        Noop,
        First,
        Second(u32),
        Third(u64),
    }

    impl Default for TestMessages {
        fn default() -> Self {
            return TestMessages::Noop;
        }
    }

    impl TestMessages {
        fn serialize(&self) -> Payload {
            bincode::serialize(&self).unwrap()
        }
    }

    #[test]
    fn send_message() {
        let config = ReliableOrderedChannelConfig::default();
        let mut channel: ReliableOrderedChannel = ReliableOrderedChannel::new(config);
        let sequence = 0;

        assert!(!channel.has_messages_to_send());
        assert_eq!(channel.num_messages_sent, 0);

        channel
            .send_message(TestMessages::Second(0).serialize())
            .unwrap();
        assert_eq!(channel.num_messages_sent, 1);
        assert!(channel.receive_message().is_none());

        let messages = channel.get_messages_to_send(u32::MAX, sequence).unwrap();

        assert_eq!(messages.len(), 1);
        assert!(channel.has_messages_to_send());

        channel.process_ack(sequence);
        assert!(!channel.has_messages_to_send());
    }

    #[test]
    fn receive_message() {
        let config = ReliableOrderedChannelConfig::default();
        let mut channel: ReliableOrderedChannel = ReliableOrderedChannel::new(config);

        let messages = vec![
            bincode::serialize(&ReliableMessage::new(0, TestMessages::First.serialize())).unwrap(),
            bincode::serialize(&ReliableMessage::new(
                1,
                TestMessages::Second(0).serialize(),
            ))
            .unwrap(),
        ];

        channel.process_messages(messages);

        let message = channel.receive_message().unwrap();
        assert_eq!(message, TestMessages::First.serialize());

        let message = channel.receive_message().unwrap();
        assert_eq!(message, TestMessages::Second(0).serialize());

        assert_eq!(channel.num_messages_received, 2);
    }

    #[test]
    fn over_budget() {
        let first_message = TestMessages::Third(0);
        let second_message = TestMessages::Third(1);

        let message = ReliableMessage::new(0, first_message.serialize());

        let config = ReliableOrderedChannelConfig::default();
        let mut channel: ReliableOrderedChannel = ReliableOrderedChannel::new(config);

        channel.send_message(first_message.serialize()).unwrap();
        channel.send_message(second_message.serialize()).unwrap();

        let message_size = bincode::serialized_size(&message).unwrap() as u32;

        let messages = channel.get_messages_to_send(message_size, 0);
        assert!(messages.is_some());
        let messages = messages.unwrap();

        assert_eq!(messages.len(), 1);

        channel.process_ack(0);

        let messages = channel.get_messages_to_send(message_size, 1);
        assert!(messages.is_some());
        let messages = messages.unwrap();

        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn resend_message() {
        let mut config = ReliableOrderedChannelConfig::default();
        config.message_resend_time = Duration::from_millis(0);
        let mut channel: ReliableOrderedChannel = ReliableOrderedChannel::new(config);

        channel
            .send_message(TestMessages::First.serialize())
            .unwrap();

        let messages = channel.get_messages_to_send(u32::MAX, 0).unwrap();
        assert_eq!(messages.len(), 1);

        let messages = channel.get_messages_to_send(u32::MAX, 1).unwrap();
        assert_eq!(messages.len(), 1);
    }
}
