use crate::{
    channel::{ReceiveChannel, SendChannel},
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
    sequence_buffer::{sequence_greater_than, sequence_less_than, SequenceBuffer},
    timer::Timer,
};

use bincode::Options;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ReliableMessage {
    id: u16,
    payload: Bytes,
}

#[derive(Debug, Clone, Default)]
pub struct ReliableMessageSent {
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
    /// Maximum size that a message can have in this channel, for reliable channel this value
    /// need to be less than the packet budget
    pub max_message_size: u64,
    /// Delay to wait before resending messages
    pub message_resend_time: Duration,
    /// If set to true, messages will be received in the order they were sent
    pub ordered: bool,
}

#[derive(Debug)]
pub(crate) struct SendReliableChannel {
    channel_id: u8,
    packet_budget: u64,
    max_message_size: u64,
    message_resend_time: Duration,
    packets_sent: SequenceBuffer<PacketSent>,
    messages_send: SequenceBuffer<ReliableMessageSent>,
    send_message_id: u16,
    num_messages_sent: u64,
    oldest_unacked_message_id: u16,
    error: Option<ChannelError>,
}

#[derive(Debug)]
enum ReceiveOrder {
    Ordered,
    Unordered {
        received_messages: SequenceBuffer<()>,
        most_recent_message_id: u16,
    },
}

#[derive(Debug)]
pub(crate) struct ReceiveReliableChannel {
    channel_id: u8,
    max_message_size: u64,
    messages_received: SequenceBuffer<ReliableMessage>,
    awaiting_message_id: u16,
    num_messages_received: u64,
    receive_order: ReceiveOrder,
    error: Option<ChannelError>,
}

impl ReliableMessage {
    fn new(id: u16, payload: Bytes) -> Self {
        Self { id, payload }
    }
}

impl ReliableMessageSent {
    fn new(reliable_message: ReliableMessage, resend_time: Duration, current_time: Duration) -> Self {
        let mut resend_timer = Timer::new(current_time, resend_time);
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
            max_message_size: 3000,
            message_resend_time: Duration::from_millis(200),
            ordered: false,
        }
    }
}

impl SendReliableChannel {
    pub fn has_messages_to_send(&self) -> bool {
        self.oldest_unacked_message_id != self.send_message_id
    }
}

impl SendReliableChannel {
    pub fn new(config: ReliableChannelConfig) -> Self {
        assert!(config.max_message_size <= config.packet_budget);

        Self {
            channel_id: config.channel_id,
            packet_budget: config.packet_budget,
            max_message_size: config.max_message_size,
            send_message_id: 0,
            oldest_unacked_message_id: 0,
            packets_sent: SequenceBuffer::with_capacity(config.sent_packet_buffer_size),
            messages_send: SequenceBuffer::with_capacity(config.message_send_queue_size),
            message_resend_time: config.message_resend_time,
            num_messages_sent: 0,
            error: None,
        }
    }
}

impl SendChannel for SendReliableChannel {
    fn get_messages_to_send(&mut self, mut available_bytes: u64, sequence: u16, current_time: Duration) -> Option<ChannelPacketData> {
        if !self.has_messages_to_send() || self.error.is_some() {
            return None;
        }

        available_bytes = available_bytes.min(self.packet_budget);
        let mut messages: Vec<Payload> = vec![];
        let mut message_ids: Vec<u16> = vec![];

        for i in 0..self.messages_send.size() {
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);
            let message_send = self.messages_send.get_mut(message_id);
            if let Some(message_send) = message_send {
                if !message_send.resend_timer.is_finished(current_time) {
                    continue;
                }

                let serialized_size = match bincode::options().serialized_size(&message_send.reliable_message) {
                    Ok(size) => size as u64,
                    Err(e) => {
                        log::error!("Failed to get message size in reliable channel {}: {}", self.channel_id, e);
                        self.error = Some(ChannelError::FailedToSerialize);
                        return None;
                    }
                };

                if serialized_size <= available_bytes {
                    available_bytes -= serialized_size;
                    message_send.resend_timer.reset(current_time);
                    message_ids.push(message_id);
                    let message = match bincode::options().serialize(&message_send.reliable_message) {
                        Ok(message) => message,
                        Err(e) => {
                            log::error!("Failed to serialize message in reliable channel {}: {}", self.channel_id, e);
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
            channel_id: self.channel_id,
            messages,
        })
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

            // Update oldest message ack
            let stop_id = self.messages_send.sequence();

            while self.oldest_unacked_message_id != stop_id && !self.messages_send.exists(self.oldest_unacked_message_id) {
                self.oldest_unacked_message_id = self.oldest_unacked_message_id.wrapping_add(1);
            }
        }
    }

    fn send_message(&mut self, payload: Bytes, current_time: Duration) {
        if self.error.is_some() {
            return;
        }

        let message_id = self.send_message_id;
        if !self.messages_send.available(message_id) {
            self.error = Some(ChannelError::ReliableChannelOutOfSync);
            return;
        }

        if payload.len() as u64 > self.max_message_size {
            log::error!(
                "Tried to send reliable message with size above the limit, got {} bytes, expected less than {}",
                payload.len(),
                self.max_message_size
            );
            self.error = Some(ChannelError::SentMessageAboveMaxSize);
            return;
        }

        self.send_message_id = self.send_message_id.wrapping_add(1);

        let reliable_message = ReliableMessage::new(message_id, payload);
        let entry = ReliableMessageSent::new(reliable_message, self.message_resend_time, current_time);
        self.messages_send.insert(message_id, entry);

        self.num_messages_sent += 1;
    }

    fn can_send_message(&self) -> bool {
        self.messages_send.available(self.send_message_id)
    }

    fn error(&self) -> Option<ChannelError> {
        self.error
    }
}

impl ReceiveReliableChannel {
    pub fn new(config: ReliableChannelConfig) -> Self {
        assert!(config.max_message_size <= config.packet_budget);
        let receive_order = match config.ordered {
            true => ReceiveOrder::Ordered,
            false => ReceiveOrder::Unordered {
                received_messages: SequenceBuffer::with_capacity(config.message_receive_queue_size),
                most_recent_message_id: 0,
            },
        };

        Self {
            channel_id: config.channel_id,
            max_message_size: config.max_message_size,
            awaiting_message_id: 0,
            num_messages_received: 0,
            messages_received: SequenceBuffer::with_capacity(config.message_receive_queue_size),
            receive_order,
            error: None,
        }
    }
}

impl ReceiveChannel for ReceiveReliableChannel {
    fn process_messages(&mut self, messages: Vec<Payload>) {
        if self.error.is_some() {
            return;
        }

        for message in messages.iter() {
            match bincode::options().deserialize::<ReliableMessage>(message) {
                Ok(message) => {
                    if message.payload.len() as u64 > self.max_message_size {
                        log::error!(
                            "Received reliable message with size above the limit, got {} bytes, expected less than {}",
                            message.payload.len(),
                            self.max_message_size
                        );
                        self.error = Some(ChannelError::ReceivedMessageAboveMaxSize);
                        return;
                    }

                    if sequence_less_than(message.id, self.awaiting_message_id) {
                        // Discard old message
                        continue;
                    }

                    let max_message_id = self.awaiting_message_id.wrapping_add(self.messages_received.size() as u16 - 1);
                    if sequence_greater_than(message.id, max_message_id) {
                        // Out of space to to add messages
                        self.error = Some(ChannelError::ReliableChannelOutOfSync);
                    }

                    match &mut self.receive_order {
                        ReceiveOrder::Ordered => {
                            if !self.messages_received.exists(message.id) {
                                self.messages_received.insert(message.id, message);
                            }
                        }
                        ReceiveOrder::Unordered {
                            ref mut received_messages,
                            ref mut most_recent_message_id,
                        } => {
                            if !received_messages.exists(message.id) {
                                received_messages.insert(message.id, ());
                                if *most_recent_message_id < message.id {
                                    *most_recent_message_id = message.id;
                                }
                                self.messages_received.insert(message.id, message);
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to deserialize reliable message in channel {}: {}", self.channel_id, e);
                    self.error = Some(ChannelError::FailedToSerialize);
                    return;
                }
            }
        }
    }

    fn receive_message(&mut self) -> Option<Payload> {
        if self.error.is_some() {
            return None;
        }

        match &mut self.receive_order {
            ReceiveOrder::Ordered => {
                let current_message_id = self.awaiting_message_id;
                if !self.messages_received.exists(current_message_id) {
                    return None;
                }

                self.awaiting_message_id = self.awaiting_message_id.wrapping_add(1);
                self.num_messages_received += 1;

                self.messages_received.remove(current_message_id).map(|m| m.payload.to_vec())
            }
            ReceiveOrder::Unordered {
                most_recent_message_id, ..
            } => {
                let mut current_message_id = self.awaiting_message_id;
                let max_received_message_id = most_recent_message_id.wrapping_add(1);
                while sequence_less_than(current_message_id, max_received_message_id) {
                    if !self.messages_received.exists(current_message_id) {
                        current_message_id = current_message_id.wrapping_add(1);
                        continue;
                    }

                    if self.awaiting_message_id == current_message_id {
                        self.awaiting_message_id = self.awaiting_message_id.wrapping_add(1);
                    }

                    self.num_messages_received += 1;
                    return self.messages_received.remove(current_message_id).map(|m| m.payload.to_vec());
                }

                None
            }
        }
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
        First,
        Second,
        Third,
    }

    impl TestMessages {
        fn serialize(&self) -> Bytes {
            bincode::options().serialize(&self).unwrap().into()
        }
    }

    #[test]
    fn send_receive_ordered_message() {
        let current_time = Duration::ZERO;
        let config = ReliableChannelConfig {
            ordered: true,
            ..Default::default()
        };
        let mut send_channel = SendReliableChannel::new(config.clone());
        let mut receive_channel = ReceiveReliableChannel::new(config);
        let sequence = 0;

        assert!(!send_channel.has_messages_to_send());
        assert_eq!(send_channel.num_messages_sent, 0);

        // Send first message
        let first_message = TestMessages::First.serialize();
        send_channel.send_message(first_message.clone(), current_time);
        let first_channel_data = send_channel.get_messages_to_send(u64::MAX, sequence, current_time).unwrap();
        assert_eq!(first_channel_data.messages.len(), 1);

        // Send second message
        let second_message = TestMessages::First.serialize();
        send_channel.send_message(second_message.clone(), current_time);
        let second_channel_data = send_channel.get_messages_to_send(u64::MAX, sequence + 1, current_time).unwrap();
        assert_eq!(second_channel_data.messages.len(), 1);

        // Process and try to receive second message
        // receive_channel.process_messages(second_channel_data.messages);
        // let received_message = receive_channel.receive_message();
        // assert!(received_message.is_none());

        // Process and receive first message
        receive_channel.process_messages(first_channel_data.messages.clone());
        let received_message = receive_channel.receive_message().unwrap();
        assert_eq!(received_message, first_message);

        receive_channel.process_messages(first_channel_data.messages);
        let received_message = receive_channel.receive_message().unwrap();


        // Now receive second message
        let received_message = receive_channel.receive_message().unwrap();
        assert_eq!(received_message, second_message);

        assert!(send_channel.has_messages_to_send());
        send_channel.process_ack(sequence);
        send_channel.process_ack(sequence + 1);
        assert!(!send_channel.has_messages_to_send());
    }

    #[test]
    fn send_receive_unordered_message() {
        let current_time = Duration::ZERO;
        let config = ReliableChannelConfig {
            ordered: false,
            ..Default::default()
        };
        let mut send_channel = SendReliableChannel::new(config.clone());
        let mut receive_channel = ReceiveReliableChannel::new(config);
        let sequence = 0;

        assert!(!send_channel.has_messages_to_send());
        assert_eq!(send_channel.num_messages_sent, 0);

        // Send first message
        let first_message = TestMessages::First.serialize();
        send_channel.send_message(first_message.clone(), current_time);
        let first_channel_data = send_channel.get_messages_to_send(u64::MAX, sequence, current_time).unwrap();
        assert_eq!(first_channel_data.messages.len(), 1);

        // Send second message
        let second_message = TestMessages::First.serialize();
        send_channel.send_message(second_message.clone(), current_time);
        let second_channel_data = send_channel.get_messages_to_send(u64::MAX, sequence + 1, current_time).unwrap();
        assert_eq!(second_channel_data.messages.len(), 1);

        // Process and receive second message
        receive_channel.process_messages(second_channel_data.messages);
        let received_message = receive_channel.receive_message().unwrap();
        assert_eq!(received_message, second_message);

        // Process and receive first message
        receive_channel.process_messages(first_channel_data.messages);
        let received_message = receive_channel.receive_message().unwrap();
        assert_eq!(received_message, first_message);

        assert!(send_channel.has_messages_to_send());
        send_channel.process_ack(sequence);
        send_channel.process_ack(sequence + 1);
        assert!(!send_channel.has_messages_to_send());
    }

    #[test]
    fn over_budget() {
        let current_time = Duration::ZERO;
        let config = ReliableChannelConfig::default();
        let mut channel = SendReliableChannel::new(config);

        let first_message = TestMessages::First.serialize();
        let second_message = TestMessages::Second.serialize();

        let message = ReliableMessage::new(0, first_message.clone());
        let message_size = bincode::options().serialized_size(&message).unwrap() as u64;

        channel.send_message(first_message, current_time);
        channel.send_message(second_message, current_time);

        let channel_data = channel.get_messages_to_send(message_size, 0, current_time).unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        channel.process_ack(0);

        let channel_data = channel.get_messages_to_send(message_size, 1, current_time).unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn resend_message() {
        let mut current_time = Duration::ZERO;
        let message_resend_time = Duration::from_millis(100);
        let config = ReliableChannelConfig {
            message_resend_time,
            ..Default::default()
        };
        let mut channel = SendReliableChannel::new(config);

        channel.send_message(TestMessages::First.serialize(), current_time);

        let channel_data = channel.get_messages_to_send(u64::MAX, 0, current_time).unwrap();
        assert_eq!(channel_data.messages.len(), 1);

        assert!(channel.get_messages_to_send(u64::MAX, 1, current_time).is_none());
        // Advance time
        current_time += message_resend_time;

        let channel_data = channel.get_messages_to_send(u64::MAX, 2, current_time).unwrap();
        assert_eq!(channel_data.messages.len(), 1);
    }

    #[test]
    fn out_of_sync() {
        let current_time = Duration::ZERO;
        let send_config = ReliableChannelConfig {
            message_send_queue_size: 2,
            ..Default::default()
        };
        let receive_config = ReliableChannelConfig {
            message_receive_queue_size: 1,
            ..Default::default()
        };
        let mut send_channel = SendReliableChannel::new(send_config);
        let mut receive_channel = ReceiveReliableChannel::new(receive_config);
        let message = TestMessages::First.serialize();

        send_channel.send_message(message.clone(), current_time);
        let first_channel_data = send_channel.get_messages_to_send(u64::MAX, 0, current_time).unwrap();
        send_channel.send_message(message.clone(), current_time);
        let second_channel_data = send_channel.get_messages_to_send(u64::MAX, 0, current_time).unwrap();

        send_channel.send_message(message, current_time);
        assert!(matches!(send_channel.error(), Some(ChannelError::ReliableChannelOutOfSync)));

        receive_channel.process_messages(first_channel_data.messages);
        receive_channel.process_messages(second_channel_data.messages);
        assert!(matches!(receive_channel.error(), Some(ChannelError::ReliableChannelOutOfSync)));
    }
}
