use crate::{
    channel::{ReceiveChannel, SendChannel},
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
    sequence_buffer::sequence_less_than,
};

use bincode::Options;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use std::{collections::VecDeque, time::Duration};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SequencedMessage {
    id: u16,
    payload: Bytes,
}

/// Configuration for a unreliable and unordered channel.
/// Messages sent in this channel can be lost and arrive in an different order that they were sent.
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
    /// If this is true, only most recent messages will be received,
    /// old messages received out of order are dropped.
    pub sequenced: bool,
}

#[derive(Debug)]
enum SendOrder {
    None,
    Sequenced { send_message_id: u16 },
}

#[derive(Debug)]
pub(crate) struct SendUnreliableChannel {
    channel_id: u8,
    packet_budget: u64,
    max_message_size: u64,
    message_send_queue_size: usize,
    messages_to_send: VecDeque<Bytes>,
    send_order: SendOrder,
    error: Option<ChannelError>,
}

#[derive(Debug)]
enum ReceiveOrder {
    None,
    Sequenced { most_recent_message_id: u16 },
}

#[derive(Debug)]
pub(crate) struct ReceiveUnreliableChannel {
    channel_id: u8,
    max_message_size: u64,
    message_receive_queue_size: usize,
    messages_received: VecDeque<Payload>,
    receive_order: ReceiveOrder,
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
            sequenced: false,
        }
    }
}

impl SendUnreliableChannel {
    pub fn new(config: UnreliableChannelConfig) -> Self {
        assert!(config.max_message_size <= config.packet_budget);
        let send_order = match config.sequenced {
            true => SendOrder::Sequenced { send_message_id: 0 },
            false => SendOrder::None,
        };

        Self {
            channel_id: config.channel_id,
            packet_budget: config.packet_budget,
            max_message_size: config.max_message_size,
            message_send_queue_size: config.message_send_queue_size,
            messages_to_send: VecDeque::with_capacity(config.message_send_queue_size),
            send_order,
            error: None,
        }
    }
}

impl SendChannel for SendUnreliableChannel {
    fn get_messages_to_send(&mut self, mut available_bytes: u64, _sequence: u16, _current_time: Duration) -> Option<ChannelPacketData> {
        if self.error.is_some() {
            return None;
        }

        let mut messages = vec![];
        available_bytes = available_bytes.min(self.packet_budget);

        while let Some(message) = self.messages_to_send.pop_front() {
            let message = match &mut self.send_order {
                SendOrder::None => message.to_vec(),
                SendOrder::Sequenced { send_message_id } => {
                    let sequenced_message = SequencedMessage {
                        id: *send_message_id,
                        payload: message,
                    };
                    *send_message_id = send_message_id.wrapping_add(1);

                    match bincode::options().serialize(&sequenced_message) {
                        Ok(message) => message,
                        Err(e) => {
                            log::error!("Failed to serialize message in unreliable channel {}: {}", self.channel_id, e);
                            self.error = Some(ChannelError::FailedToSerialize);
                            return None;
                        }
                    }
                }
            };

            let message_size = message.len() as u64;
            if message_size > available_bytes {
                // No available bytes, drop message
                continue;
            }

            messages.push(message);
            available_bytes -= message_size;
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

    fn send_message(&mut self, payload: Bytes, _current_time: Duration) {
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

        let receive_order = match config.sequenced {
            true => ReceiveOrder::Sequenced { most_recent_message_id: 0 },
            false => ReceiveOrder::None,
        };

        // Sequenced messages have an additional u16 id
        let max_message_size = match config.sequenced {
            true => config.max_message_size + 2,
            false => config.max_message_size,
        };

        Self {
            channel_id: config.channel_id,
            max_message_size,
            message_receive_queue_size: config.message_receive_queue_size,
            messages_received: VecDeque::with_capacity(config.message_receive_queue_size),
            receive_order,
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

            let message = match &mut self.receive_order {
                ReceiveOrder::None => message,
                ReceiveOrder::Sequenced { most_recent_message_id } => match bincode::options().deserialize::<SequencedMessage>(&message) {
                    Ok(sequenced_message) => {
                        if sequence_less_than(sequenced_message.id, *most_recent_message_id) {
                            continue;
                        }
                        *most_recent_message_id = sequenced_message.id;

                        sequenced_message.payload.to_vec()
                    }
                    Err(e) => {
                        log::error!("Failed to deserialize unreliable message in channel {}: {}", self.channel_id, e);
                        self.error = Some(ChannelError::FailedToSerialize);
                        return;
                    }
                },
            };

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn send_receive_unreliable_message() {
        let current_time = Duration::ZERO;
        let config = UnreliableChannelConfig {
            sequenced: false,
            ..Default::default()
        };
        let mut send_channel = SendUnreliableChannel::new(config.clone());
        let mut receive_channel = ReceiveUnreliableChannel::new(config);
        let sequence = 0;

        // Send first message
        let first_message = Bytes::from(vec![0]);
        send_channel.send_message(first_message.clone(), current_time);
        let first_channel_data = send_channel.get_messages_to_send(u64::MAX, sequence, current_time).unwrap();
        assert_eq!(first_channel_data.messages.len(), 1);

        // Send second message
        let second_message = Bytes::from(vec![1]);
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
    }

    #[test]
    fn send_receive_sequenced_message() {
        let current_time = Duration::ZERO;
        let config = UnreliableChannelConfig {
            sequenced: true,
            ..Default::default()
        };
        let mut send_channel = SendUnreliableChannel::new(config.clone());
        let mut receive_channel = ReceiveUnreliableChannel::new(config);
        let sequence = 0;

        // Send first message
        let first_message = Bytes::from(vec![0]);
        send_channel.send_message(first_message, current_time);
        let first_channel_data = send_channel.get_messages_to_send(u64::MAX, sequence, current_time).unwrap();
        assert_eq!(first_channel_data.messages.len(), 1);

        // Send second message
        let second_message = Bytes::from(vec![1]);
        send_channel.send_message(second_message.clone(), current_time);
        let second_channel_data = send_channel.get_messages_to_send(u64::MAX, sequence + 1, current_time).unwrap();
        assert_eq!(second_channel_data.messages.len(), 1);

        // Process and receive second message
        receive_channel.process_messages(second_channel_data.messages);
        let received_message = receive_channel.receive_message().unwrap();
        assert_eq!(received_message, second_message);

        // Process and don't receive first message
        receive_channel.process_messages(first_channel_data.messages);
        let received_message = receive_channel.receive_message();
        assert!(received_message.is_none());
    }
}
