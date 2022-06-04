use crate::{
    channel::Channel,
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
    sequence_buffer::SequenceBuffer,
    timer::Timer,
};

use bincode::Options;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use std::time::Duration;

use super::ChannelNetworkInfo;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ReliableMessage {
    id: u16,
    payload: Bytes,
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

/// Configuration for a reliable and ordered channel.
/// Messages will be received in the order they were sent.
/// If a message is lost it'll be resent.
#[derive(Debug, Clone)]
pub struct ReliableChannelConfig {
    /// Channel identifier, unique between all channels
    pub channel_id: u8,
    /// Number of packet entries in the sent packet sequence buffer.
    /// Consider a few seconds of worth of entries in this buffer, based on your packet send rate
    pub sent_packet_buffer_size: usize,
    /// Allowed numbers of messages in the send queue for this channel
    pub message_send_queue_size: usize,
    /// Allowed numbers of messages in the receive queue for this channel
    pub message_receive_queue_size: usize,
    /// Maximum nuber of bytes that this channel is allowed to write per packet
    pub packet_budget: u64,
    /// Delay to wait before resending messages
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
    info: ChannelNetworkInfo,
    error: Option<ChannelError>,
}

impl ReliableMessage {
    pub fn new(id: u16, payload: Bytes) -> Self {
        Self { id, payload }
    }
}

impl ReliableMessageSent {
    pub fn new(reliable_message: ReliableMessage, resend_time: Duration) -> Self {
        let mut resend_timer = Timer::new(resend_time);
        resend_timer.finish();
        Self {
            reliable_message,
            resend_timer,
        }
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
            packet_budget: 6000,
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
            info: ChannelNetworkInfo::default(),
            error: None,
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
}

impl Channel for ReliableChannel {
    fn advance_time(&mut self, duration: Duration) {
        self.info.reset();
        let message_limit = self.config.message_send_queue_size;
        for i in 0..message_limit {
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);
            if let Some(message) = self.messages_send.get_mut(message_id) {
                message.resend_timer.advance(duration);
            }
        }
    }

    fn get_messages_to_send(&mut self, mut available_bytes: u64, sequence: u16) -> Option<ChannelPacketData> {
        if !self.has_messages_to_send() || self.error.is_some() {
            return None;
        }

        available_bytes = available_bytes.min(self.config.packet_budget);
        let mut messages: Vec<Payload> = vec![];
        let mut message_ids: Vec<u16> = vec![];
        let message_limit = self.config.message_send_queue_size;

        for i in 0..message_limit {
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);
            let message_send = self.messages_send.get_mut(message_id);
            if let Some(message_send) = message_send {
                if !message_send.resend_timer.is_finished() {
                    continue;
                }

                let serialized_size = match bincode::options().serialized_size(&message_send.reliable_message) {
                    Ok(size) => size as u64,
                    Err(e) => {
                        log::error!("Failed to get message size in channel {}: {}", self.config.channel_id, e);
                        self.error = Some(ChannelError::FailedToSerialize);
                        return None;
                    }
                };

                if serialized_size <= available_bytes {
                    available_bytes -= serialized_size;
                    message_send.resend_timer.reset();
                    message_ids.push(message_id);
                    self.info.messages_sent += 1;
                    self.info.bytes_sent += serialized_size;
                    let message = match bincode::options().serialize(&message_send.reliable_message) {
                        Ok(message) => message,
                        Err(e) => {
                            log::error!("Failed to serialize message in channel {}: {}", self.config.channel_id, e);
                            self.error = Some(ChannelError::FailedToSerialize);
                            return None;
                        }
                    };
                    messages.push(message);
                }
            }
        }

        if messages.is_empty() {
            return None;
        }

        let packet_sent = PacketSent::new(message_ids);
        self.packets_sent.insert(sequence, packet_sent);

        Some(ChannelPacketData {
            channel_id: self.config.channel_id,
            messages,
        })
    }

    fn process_messages(&mut self, messages: Vec<Payload>) {
        if self.error.is_some() {
            return;
        }

        for message in messages.iter() {
            self.info.bytes_received += message.len() as u64;
            self.info.messages_received += 1;
            match bincode::options().deserialize::<ReliableMessage>(message) {
                Ok(message) => {
                    if !self.messages_received.exists(message.id) {
                        self.messages_received.insert(message.id, message);
                    }
                }
                Err(e) => {
                    log::error!("Failed to deserialize reliable message {}: {}", self.config.channel_id, e);
                    self.error = Some(ChannelError::FailedToSerialize);
                    return;
                }
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

    fn send_message(&mut self, payload: Bytes) {
        if self.error.is_some() {
            return;
        }

        let message_id = self.send_message_id;
        if !self.messages_send.available(message_id) {
            self.error = Some(ChannelError::ReliableChannelOutOfSync);
            return;
        }

        if payload.len() as u64 > self.config.packet_budget {
            self.error = Some(ChannelError::MessageAbovePacketBudget);
            return;
        }

        self.send_message_id = self.send_message_id.wrapping_add(1);

        let reliable_message = ReliableMessage::new(message_id, payload);
        let entry = ReliableMessageSent::new(reliable_message, self.config.message_resend_time);
        self.messages_send.insert(message_id, entry);

        self.num_messages_sent += 1;
    }

    fn receive_message(&mut self) -> Option<Payload> {
        if self.error.is_some() {
            return None;
        }

        let received_message_id = self.received_message_id;

        if !self.messages_received.exists(received_message_id) {
            return None;
        }

        self.received_message_id = self.received_message_id.wrapping_add(1);
        self.num_messages_received += 1;

        self.messages_received.remove(received_message_id).map(|m| m.payload.to_vec())
    }

    fn can_send_message(&self) -> bool {
        self.messages_send.available(self.send_message_id)
    }

    fn channel_network_info(&self) -> ChannelNetworkInfo {
        self.info
    }

    fn error(&self) -> Option<ChannelError> {
        self.error
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
        fn serialize(&self) -> Bytes {
            bincode::options().serialize(&self).unwrap().into()
        }
    }

    #[test]
    fn send_message() {
        let config = ReliableChannelConfig::default();
        let mut channel: ReliableChannel = ReliableChannel::new(config);
        let sequence = 0;

        assert!(!channel.has_messages_to_send());
        assert_eq!(channel.num_messages_sent, 0);

        channel.send_message(TestMessages::Second(0).serialize().into());
        assert_eq!(channel.num_messages_sent, 1);
        assert!(channel.receive_message().is_none());

        let channel_data = channel.get_messages_to_send(u64::MAX, sequence).unwrap();

        assert_eq!(channel_data.messages.len(), 1);
        assert_eq!(
            channel_data,
            ChannelPacketData {
                channel_id: 0,
                messages: vec![bincode::options()
                    .serialize(&ReliableMessage::new(0, TestMessages::Second(0).serialize()))
                    .unwrap()]
            }
        );
        assert!(channel.has_messages_to_send());

        channel.process_ack(sequence);
        assert!(!channel.has_messages_to_send());
    }

    #[test]
    fn receive_message() {
        let config = ReliableChannelConfig::default();
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        let messages = vec![
            bincode::options()
                .serialize(&ReliableMessage::new(0, TestMessages::First.serialize()))
                .unwrap(),
            bincode::options()
                .serialize(&ReliableMessage::new(1, TestMessages::Second(0).serialize()))
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

        let config = ReliableChannelConfig::default();
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(first_message.serialize());
        channel.send_message(second_message.serialize());

        let message_size = bincode::options().serialized_size(&message).unwrap() as u64;

        let channel_data = channel.get_messages_to_send(message_size, 0).unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        channel.process_ack(0);

        let channel_data = channel.get_messages_to_send(message_size, 1).unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn resend_message() {
        let mut config = ReliableChannelConfig::default();
        config.message_resend_time = Duration::from_millis(100);
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(TestMessages::First.serialize());

        let channel_data = channel.get_messages_to_send(u64::MAX, 0).unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        assert!(channel.get_messages_to_send(u64::MAX, 1).is_none());
        channel.advance_time(Duration::from_millis(100));

        let channel_data = channel.get_messages_to_send(u64::MAX, 2).unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn out_of_sync() {
        let config = ReliableChannelConfig {
            message_send_queue_size: 1,
            ..Default::default()
        };
        let mut channel: ReliableChannel = ReliableChannel::new(config);

        channel.send_message(TestMessages::Second(0).serialize());
        channel.send_message(TestMessages::Second(0).serialize());
        assert!(matches!(channel.error(), Some(ChannelError::ReliableChannelOutOfSync)));
    }
}
