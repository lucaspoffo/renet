use crate::error::{DisconnectionReason, RechannelError};
use crate::packet::{Payload, ReliableChannelData};
use crate::sequence_buffer::SequenceBuffer;
use crate::timer::Timer;

use bincode::Options;
use serde::{Deserialize, Serialize};

use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ReliableMessage {
    id: u16,
    payload: Payload,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReliableMessageSent {
    reliable_message: ReliableMessage,
    resend_timer: Timer,
}

#[derive(Debug, Clone)]
struct PacketSent {
    acked: bool,
    messages_id: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct ReliableChannelConfig {
    pub channel_id: u8,
    pub sent_packet_buffer_size: usize,
    pub message_send_queue_size: usize,
    pub message_receive_queue_size: usize,
    pub max_message_size: u64,
    pub packet_budget: u64,
    pub message_resend_time: Duration,
}

#[derive(Debug)]
pub(crate) struct ReliableChannel {
    config: ReliableChannelConfig,
    packets_sent: SequenceBuffer<PacketSent>,
    messages_send: SequenceBuffer<ReliableMessageSent>,
    messages_received: SequenceBuffer<ReliableMessage>,
    send_message_id: u16,
    received_message_id: u16,
    num_messages_sent: u64,
    num_messages_received: u64,
    oldest_unacked_message_id: u16,
    out_of_sync: bool,
}

impl ReliableMessage {
    pub fn new(id: u16, payload: Payload) -> Self {
        Self { id, payload }
    }
}

impl ReliableMessageSent {
    pub fn new(reliable_message: ReliableMessage, resend_time: Duration) -> Self {
        let mut resend_timer = Timer::new(resend_time);
        resend_timer.finish();
        Self { reliable_message, resend_timer }
    }
}

impl PacketSent {
    pub fn new(messages_id: Vec<u16>) -> Self {
        Self { acked: false, messages_id }
    }
}

impl Default for ReliableChannelConfig {
    fn default() -> Self {
        Self {
            channel_id: 0,
            sent_packet_buffer_size: 1024,
            message_send_queue_size: 1024,
            message_receive_queue_size: 1024,
            max_message_size: 1200,
            packet_budget: 2000,
            message_resend_time: Duration::from_millis(100),
        }
    }
}

impl ReliableChannel {
    pub fn new(config: ReliableChannelConfig) -> Self {
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
            out_of_sync: false,
        }
    }

    pub fn advance_time(&mut self, duration: Duration) {
        let message_limit = self.config.message_send_queue_size;
        for i in 0..message_limit {
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);
            if let Some(message) = self.messages_send.get_mut(message_id) {
                message.resend_timer.advance(duration);
            }
        }
    }

    pub fn has_messages_to_send(&self) -> bool {
        self.oldest_unacked_message_id != self.send_message_id
    }

    fn update_oldest_message_ack(&mut self) {
        let stop_id = self.messages_send.sequence();

        while self.oldest_unacked_message_id != stop_id && !self.messages_send.exists(self.oldest_unacked_message_id) {
            self.oldest_unacked_message_id = self.oldest_unacked_message_id.wrapping_add(1);
        }
    }

    pub fn get_messages_to_send(&mut self, mut available_bytes: u64, sequence: u16) -> Result<Option<ReliableChannelData>, bincode::Error> {
        if !self.has_messages_to_send() {
            return Ok(None);
        }

        available_bytes = available_bytes.min(self.config.packet_budget);

        let message_limit = self.config.message_send_queue_size;

        let mut messages = vec![];
        for i in 0..message_limit {
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);
            let message_send = self.messages_send.get_mut(message_id);
            if let Some(message_send) = message_send {
                if !message_send.resend_timer.is_finished() {
                    continue;
                }

                let serialized_size = bincode::options().serialized_size(&message_send.reliable_message)? as u64;

                if serialized_size <= available_bytes {
                    available_bytes -= serialized_size;
                    message_send.resend_timer.reset();
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
        Ok(Some(ReliableChannelData { channel_id: self.config.channel_id, messages }))
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

    pub fn send_message(&mut self, message_payload: Payload) -> Result<(), RechannelError> {
        let message_id = self.send_message_id;
        if !self.messages_send.available(message_id) {
            self.out_of_sync = true;
            let reason = DisconnectionReason::ReliableChannelOutOfSync(self.config.channel_id);
            return Err(RechannelError::ClientDisconnected(reason));
        }

        if message_payload.len() as u64 > self.config.max_message_size {
            return Err(RechannelError::MessageSizeAboveLimit);
        }
        self.send_message_id = self.send_message_id.wrapping_add(1);

        let entry = ReliableMessageSent::new(ReliableMessage::new(message_id, message_payload), self.config.message_resend_time);
        self.messages_send.insert(message_id, entry);

        self.num_messages_sent += 1;
        Ok(())
    }

    pub fn receive_message(&mut self) -> Option<Payload> {
        let received_message_id = self.received_message_id;

        if !self.messages_received.exists(received_message_id) {
            return None;
        }

        self.received_message_id = self.received_message_id.wrapping_add(1);
        self.num_messages_received += 1;

        self.messages_received.remove(received_message_id).map(|m| m.payload)
    }

    pub fn out_of_sync(&self) -> bool {
        self.out_of_sync
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
            TestMessages::Noop
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

        channel.send_message(TestMessages::Second(0).serialize()).unwrap();
        assert_eq!(channel.num_messages_sent, 1);
        assert!(channel.receive_message().is_none());

        let channel_data = channel.get_messages_to_send(u64::MAX, sequence).unwrap().unwrap();

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

        channel.send_message(first_message.serialize()).unwrap();
        channel.send_message(second_message.serialize()).unwrap();

        let message_size = bincode::options().serialized_size(&message).unwrap() as u64;

        let channel_data = channel.get_messages_to_send(message_size, 0).unwrap().unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        channel.process_ack(0);

        let channel_data = channel.get_messages_to_send(message_size, 1).unwrap().unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn resend_message() {
        let mut config = ReliableChannelConfig::default();
        config.message_resend_time = Duration::from_millis(100);
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(TestMessages::First.serialize()).unwrap();

        let channel_data = channel.get_messages_to_send(u64::MAX, 0).unwrap().unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        assert!(channel.get_messages_to_send(u64::MAX, 1).unwrap().is_none());
        channel.advance_time(Duration::from_millis(100));

        let channel_data = channel.get_messages_to_send(u64::MAX, 2).unwrap().unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn out_of_sync() {
        let config = ReliableChannelConfig { message_send_queue_size: 1, ..Default::default() };
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(TestMessages::Second(0).serialize()).unwrap();

        assert!(channel.send_message(TestMessages::Second(0).serialize()).is_err());
        assert!(matches!(channel.out_of_sync(), true));
    }
}
