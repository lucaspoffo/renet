use crate::packet::ReliableChannelData;
use crate::sequence_buffer::SequenceBuffer;
use crate::{error::RenetError, packet::Payload};

use bincode::Options;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::time::{Duration, Instant};

#[derive(Debug, Error)]
#[error("Reliable Channel is out of sync, a message was dropped")]
pub struct OutOfSync;

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
pub struct ReliableChannelConfig {
    pub channel_id: u8,
    pub sent_packet_buffer_size: usize,
    pub message_queue_size: usize,
    pub packet_budget: u64,
    pub message_resend_time: Duration,
}

impl Default for ReliableChannelConfig {
    fn default() -> Self {
        Self {
            channel_id: 0,
            sent_packet_buffer_size: 1024,
            message_queue_size: 1024,
            packet_budget: 1200,
            message_resend_time: Duration::from_millis(100),
        }
    }
}

pub struct ReliableChannel {
    config: ReliableChannelConfig,
    packets_sent: SequenceBuffer<PacketSent>,
    messages_send: SequenceBuffer<ReliableMessageSent>,
    messages_received: SequenceBuffer<ReliableMessage>,
    send_message_id: u16,
    received_message_id: u16,
    num_messages_sent: u64,
    num_messages_received: u64,
    oldest_unacked_message_id: u16,
    error: Option<OutOfSync>,
}

impl ReliableChannel {
    pub fn new(config: ReliableChannelConfig) -> Self {
        Self {
            packets_sent: SequenceBuffer::with_capacity(config.sent_packet_buffer_size),
            messages_send: SequenceBuffer::with_capacity(config.message_queue_size),
            messages_received: SequenceBuffer::with_capacity(config.message_queue_size),
            send_message_id: 0,
            received_message_id: 0,
            num_messages_received: 0,
            num_messages_sent: 0,
            oldest_unacked_message_id: 0,
            config,
            error: None,
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

    pub fn get_messages_to_send(
        &mut self,
        mut available_bytes: u64,
        sequence: u16,
    ) -> Result<Option<ReliableChannelData>, RenetError> {
        if !self.has_messages_to_send() {
            return Ok(None);
        }

        available_bytes = available_bytes.min(self.config.packet_budget);

        let message_limit = self.config.message_queue_size;

        let mut messages = vec![];
        let now = Instant::now();
        for i in 0..message_limit {
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);
            let message_send = self.messages_send.get_mut(message_id);
            if let Some(message_send) = message_send {
                let send = match message_send.last_time_sent {
                    Some(last_time_sent) => {
                        (last_time_sent + self.config.message_resend_time) <= now
                    }
                    None => true,
                };

                let serialized_size =
                    bincode::options().serialized_size(&message_send.reliable_message)? as u64;

                if send && serialized_size <= available_bytes {
                    available_bytes -= serialized_size;
                    message_send.last_time_sent = Some(now);

                    messages.push(message_send.reliable_message.clone());
                }
            }
        }

        if messages.is_empty() {
            return Ok(None);
        }

        let messages_id: Vec<u16> = messages.iter().map(|m| m.id).collect();

        let packet_sent = PacketSent::new(messages_id);
        self.packets_sent.insert(sequence, packet_sent);
        Ok(Some(ReliableChannelData::new(
            self.config.channel_id,
            messages,
        )))
    }

    pub fn process_messages(&mut self, messages: Vec<ReliableMessage>) {
        for message in messages.into_iter() {
            if !self.messages_received.exists(message.id) {
                self.messages_received.insert(message.id, message);
            }
        }
    }

    pub fn process_ack(&mut self, ack: u16) {
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

    pub fn send_message(&mut self, message_payload: Payload) {
        let message_id = self.send_message_id;
        if !self.messages_send.available(message_id) {
            self.error = Some(OutOfSync);
            return;
        }
        self.send_message_id = self.send_message_id.wrapping_add(1);

        let entry = ReliableMessageSent::new(ReliableMessage::new(message_id, message_payload));
        self.messages_send.insert(message_id, entry);

        self.num_messages_sent += 1;
    }

    pub fn receive_message(&mut self) -> Option<Payload> {
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

    pub fn error(&self) -> Option<&OutOfSync> {
        self.error.as_ref()
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
            bincode::options().serialize(&self).unwrap()
        }
    }

    #[test]
    fn send_message() {
        let config = ReliableChannelConfig::default();
        let mut channel: ReliableChannel = ReliableChannel::new(config);
        let sequence = 0;

        assert!(!channel.has_messages_to_send());
        assert_eq!(channel.num_messages_sent, 0);

        channel.send_message(TestMessages::Second(0).serialize());
        assert_eq!(channel.num_messages_sent, 1);
        assert!(channel.receive_message().is_none());

        let channel_data = channel
            .get_messages_to_send(u64::MAX, sequence)
            .unwrap()
            .unwrap();

        assert_eq!(channel_data.messages.len(), 1);
        assert!(channel.has_messages_to_send());

        channel.process_ack(sequence);
        assert!(!channel.has_messages_to_send());
    }

    #[test]
    fn receive_message() {
        let config = ReliableChannelConfig::default();
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        let messages = vec![
            ReliableMessage::new(0, TestMessages::First.serialize()),
            ReliableMessage::new(1, TestMessages::Second(0).serialize()),
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

        let config = ReliableChannelConfig::default();
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(first_message.serialize());
        channel.send_message(second_message.serialize());

        let message_size = bincode::options().serialized_size(&message).unwrap() as u64;

        let channel_data = channel
            .get_messages_to_send(message_size, 0)
            .unwrap()
            .unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        channel.process_ack(0);

        let channel_data = channel
            .get_messages_to_send(message_size, 1)
            .unwrap()
            .unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn resend_message() {
        let mut config = ReliableChannelConfig::default();
        config.message_resend_time = Duration::from_millis(0);
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(TestMessages::First.serialize());

        let channel_data = channel.get_messages_to_send(u64::MAX, 0).unwrap().unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        let channel_data = channel.get_messages_to_send(u64::MAX, 1).unwrap().unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn out_of_sync() {
        let config = ReliableChannelConfig {
            message_queue_size: 1,
            ..Default::default()
        };
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(TestMessages::Second(0).serialize());

        channel.send_message(TestMessages::Second(0).serialize());
        assert!(matches!(channel.error(), Some(&OutOfSync)));
    }
}
