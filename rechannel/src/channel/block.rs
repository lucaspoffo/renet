use std::{collections::VecDeque, mem, time::Duration};

use bincode::Options;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::{
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
    sequence_buffer::SequenceBuffer,
    timer::Timer,
};
use log::{error, info};

use super::{Channel, ChannelNetworkInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SliceMessage {
    chunk_id: u16,
    slice_id: u32,
    num_slices: u32,
    data: Payload,
}

#[derive(Debug, Clone)]
struct PacketSent {
    acked: bool,
    chunk_id: u16,
    slice_ids: Vec<u32>,
}

/// Configuration for a block channel, used for sending big and reliable messages,
/// that are not so frequent. Level initialization as an example.
#[derive(Debug, Clone)]
pub struct BlockChannelConfig {
    /// Channel identifier, unique between all channels
    pub channel_id: u8,
    /// Data is sliced up into fragments of this size (bytes)
    pub slice_size: usize,
    /// Delay to wait before resending messages
    pub resend_time: Duration,
    /// Number of packet entries in the sent packet sequence buffer.
    /// Consider a few seconds of worth of entries in this buffer, based on your packet send rate
    pub sent_packet_buffer_size: usize,
    /// Maximum nuber of bytes that this channel is allowed to write per packet
    pub packet_budget: u64,
    // TODO: remove queue, add can_send() -> bool
    pub message_send_queue_size: usize,
}

#[derive(Debug)]
struct ChunkSender {
    sending: bool,
    chunk_id: u16,
    slice_size: usize,
    num_slices: usize,
    current_slice_id: usize,
    num_acked_slices: usize,
    acked: Vec<bool>,
    chunk_data: Bytes,
    resend_timers: Vec<Timer>,
    packets_sent: SequenceBuffer<PacketSent>,
    resend_time: Duration,
    packet_budget: u64,
}

#[derive(Debug)]
struct ChunkReceiver {
    receiving: bool,
    chunk_id: u16,
    slice_size: usize,
    num_slices: usize,
    num_received_slices: usize,
    received: Vec<bool>,
    chunk_data: Payload,
}

#[derive(Debug)]
pub(crate) struct BlockChannel {
    channel_id: u8,
    sender: ChunkSender,
    receiver: ChunkReceiver,
    messages_received: VecDeque<Payload>,
    messages_to_send: VecDeque<Bytes>,
    message_send_queue_size: usize,
    info: ChannelNetworkInfo,
    error: Option<ChannelError>,
}

impl Default for BlockChannelConfig {
    fn default() -> Self {
        Self {
            channel_id: 2,
            slice_size: 400,
            resend_time: Duration::from_millis(300),
            sent_packet_buffer_size: 256,
            packet_budget: 4000,
            message_send_queue_size: 8,
        }
    }
}

impl PacketSent {
    fn new(chunk_id: u16, slice_ids: Vec<u32>) -> Self {
        Self {
            chunk_id,
            slice_ids,
            acked: false,
        }
    }
}

impl ChunkSender {
    fn new(slice_size: usize, sent_packet_buffer_size: usize, resend_time: Duration, packet_budget: u64) -> Self {
        Self {
            sending: false,
            chunk_id: 0,
            slice_size,
            num_slices: 0,
            current_slice_id: 0,
            num_acked_slices: 0,
            acked: Vec::new(),
            chunk_data: Bytes::new(),
            resend_timers: Vec::with_capacity(sent_packet_buffer_size),
            packets_sent: SequenceBuffer::with_capacity(sent_packet_buffer_size),
            resend_time,
            packet_budget,
        }
    }

    fn send_message(&mut self, data: Bytes) {
        assert!(!self.sending);

        self.sending = true;
        self.num_acked_slices = 0;
        self.num_slices = (data.len() + self.slice_size - 1) / self.slice_size;

        self.acked = vec![false; self.num_slices];
        let mut resend_timer = Timer::new(self.resend_time);
        resend_timer.finish();
        self.resend_timers.clear();
        self.resend_timers.resize(self.num_slices, resend_timer);
        self.chunk_data = data;
    }

    fn generate_slice_packets(&mut self, mut available_bytes: u64) -> Result<Vec<SliceMessage>, bincode::Error> {
        let mut slice_messages: Vec<SliceMessage> = vec![];
        if !self.sending {
            return Ok(slice_messages);
        }

        available_bytes = available_bytes.min(self.packet_budget);

        for i in 0..self.num_slices {
            let slice_id = (self.current_slice_id + i) % self.num_slices;

            if self.acked[slice_id] {
                continue;
            }
            let resend_timer = &mut self.resend_timers[slice_id];
            if !resend_timer.is_finished() {
                continue;
            }

            let start = slice_id * self.slice_size;
            let end = if slice_id == self.num_slices - 1 { self.chunk_data.len() } else { (slice_id + 1) * self.slice_size };

            let data = self.chunk_data[start..end].to_vec();

            let message = SliceMessage {
                chunk_id: self.chunk_id,
                slice_id: slice_id as u32,
                num_slices: self.num_slices as u32,
                data,
            };

            let message_size = bincode::options().serialized_size(&message)?;
            let message_size = message_size as u64;

            if available_bytes < message_size {
                break;
            }

            available_bytes -= message_size;
            resend_timer.reset();

            info!(
                "Generated SliceMessage {} from chunk_id {}. ({}/{})",
                message.slice_id, self.chunk_id, message.slice_id, self.num_slices
            );

            slice_messages.push(message);
        }
        self.current_slice_id = (self.current_slice_id + slice_messages.len()) % self.num_slices;

        Ok(slice_messages)
    }

    fn process_ack(&mut self, ack: u16) {
        if let Some(sent_packet) = self.packets_sent.get_mut(ack) {
            if sent_packet.acked || sent_packet.chunk_id != self.chunk_id {
                return;
            }
            sent_packet.acked = true;

            for &slice_id in sent_packet.slice_ids.iter() {
                if !self.acked[slice_id as usize] {
                    self.acked[slice_id as usize] = true;
                    self.num_acked_slices += 1;
                    info!(
                        "Acked SliceMessage {} from chunk_id {}. ({}/{})",
                        slice_id, self.chunk_id, self.num_acked_slices, self.num_slices
                    );
                }
            }

            if self.num_acked_slices == self.num_slices {
                self.sending = false;
                info!("Finished sending block message {}.", self.chunk_id);
                self.chunk_id += 1;
            }
        }
    }
}

impl ChunkReceiver {
    fn new(slice_size: usize) -> Self {
        Self {
            receiving: false,
            chunk_id: 0,
            slice_size,
            num_slices: 0,
            num_received_slices: 0,
            received: Vec::new(),
            chunk_data: Vec::new(),
        }
    }

    fn process_slice_message(&mut self, message: &SliceMessage) -> Option<Payload> {
        if !self.receiving {
            self.receiving = true;
            self.num_slices = message.num_slices as usize;
            self.chunk_id = message.chunk_id;
            self.num_received_slices = 0;
            self.received = vec![false; self.num_slices];
            info!(
                "Receiving Block message with id {} with {} slices.",
                message.chunk_id, message.num_slices
            );
            self.chunk_data = vec![0; self.num_slices * self.slice_size];
        }

        if message.chunk_id != self.chunk_id {
            error!(
                "Invalid chunk id for SliceMessage, expected {}, got {}.",
                self.chunk_id, message.chunk_id
            );
            return None;
        }

        if message.num_slices != self.num_slices as u32 {
            error!(
                "Invalid number of slices for SliceMessage, got {}, expected {}.",
                message.num_slices, self.num_slices,
            );
            return None;
        }
        let slice_id = message.slice_id as usize;
        let is_last_slice = slice_id == self.num_slices - 1;
        if is_last_slice {
            if message.data.len() > self.slice_size {
                error!(
                    "Invalid last slice_size for SliceMessage, got {}, expected < {}.",
                    message.data.len(),
                    self.slice_size,
                );
                return None;
            }
        } else if message.data.len() != self.slice_size {
            error!(
                "Invalid slice_size for SliceMessage, expected {}, got {}.",
                self.slice_size,
                message.data.len()
            );
            return None;
        }

        if !self.received[slice_id] {
            self.received[slice_id] = true;
            self.num_received_slices += 1;

            if is_last_slice {
                let len = (self.num_slices - 1) * self.slice_size + message.data.len();
                self.chunk_data.resize(len, 0);
            }

            let start = slice_id * self.slice_size;
            let end = if slice_id == self.num_slices - 1 {
                (self.num_slices - 1) * self.slice_size + message.data.len()
            } else {
                (slice_id + 1) * self.slice_size
            };

            self.chunk_data[start..end].copy_from_slice(&message.data);
            info!(
                "Received slice {} from chunk {}. ({}/{})",
                slice_id, self.chunk_id, self.num_received_slices, self.num_slices
            );
        }

        if self.num_received_slices == self.num_slices {
            info!("Received all slices for chunk {}.", self.chunk_id);
            let block = mem::take(&mut self.chunk_data);
            self.receiving = false;
            return Some(block);
        }

        None
    }
}

impl BlockChannel {
    pub fn new(config: BlockChannelConfig) -> Self {
        assert!((config.slice_size as u64) <= config.packet_budget);

        let sender = ChunkSender::new(
            config.slice_size,
            config.sent_packet_buffer_size,
            config.resend_time,
            config.packet_budget,
        );
        let receiver = ChunkReceiver::new(config.slice_size);

        Self {
            channel_id: config.channel_id,
            sender,
            receiver,
            messages_received: VecDeque::new(),
            messages_to_send: VecDeque::with_capacity(config.message_send_queue_size),
            message_send_queue_size: config.message_send_queue_size,
            info: ChannelNetworkInfo::default(),
            error: None,
        }
    }
}

impl Channel for BlockChannel {
    fn get_messages_to_send(&mut self, available_bytes: u64, sequence: u16) -> Option<ChannelPacketData> {
        if !self.sender.sending {
            if let Some(message) = self.messages_to_send.pop_front() {
                self.send_message(message);
            }
        }

        let slice_messages: Vec<SliceMessage> = match self.sender.generate_slice_packets(available_bytes) {
            Ok(messages) => messages,
            Err(e) => {
                log::error!("Failed serialize message in block channel {}: {}", self.channel_id, e);
                self.error = Some(ChannelError::FailedToSerialize);
                return None;
            }
        };

        if slice_messages.is_empty() {
            return None;
        }

        let mut messages = vec![];
        let mut slice_ids = vec![];
        for message in slice_messages.iter() {
            let slice_id = message.slice_id;
            match bincode::options().serialize(message) {
                Ok(message) => {
                    self.info.messages_sent += 1;
                    self.info.bytes_sent += message.len() as u64;
                    slice_ids.push(slice_id);
                    messages.push(message);
                }
                Err(e) => {
                    error!("Failed to serialize message in block message {}: {}", self.channel_id, e);
                    self.error = Some(ChannelError::FailedToSerialize);
                    return None;
                }
            }
        }

        let packet_sent = PacketSent::new(self.sender.chunk_id, slice_ids);
        self.sender.packets_sent.insert(sequence, packet_sent);

        Some(ChannelPacketData {
            channel_id: self.channel_id,
            messages,
        })
    }

    fn advance_time(&mut self, duration: Duration) {
        for timer in self.sender.resend_timers.iter_mut() {
            timer.advance(duration);
        }
    }

    fn process_messages(&mut self, messages: Vec<Payload>) {
        if self.error.is_some() {
            return;
        }

        for message in messages.iter() {
            match bincode::options().deserialize::<SliceMessage>(message) {
                Ok(message) => {
                    if let Some(block) = self.receiver.process_slice_message(&message) {
                        self.messages_received.push_back(block);
                    }
                }
                Err(e) => {
                    error!("Failed to deserialize slice message in channel {}: {}", self.channel_id, e);
                    self.error = Some(ChannelError::FailedToSerialize);
                    return;
                }
            }
        }
    }

    fn process_ack(&mut self, ack: u16) {
        self.sender.process_ack(ack);
    }

    fn send_message(&mut self, payload: Bytes) {
        if self.error.is_some() {
            return;
        }

        if self.sender.sending {
            if self.messages_to_send.len() >= self.message_send_queue_size {
                self.error = Some(ChannelError::SendQueueFull);
                return;
            }
            self.messages_to_send.push_back(payload);
            return;
        }

        self.sender.send_message(payload);
    }

    fn receive_message(&mut self) -> Option<Payload> {
        self.messages_received.pop_front()
    }

    fn can_send_message(&self) -> bool {
        self.messages_to_send.len() < self.message_send_queue_size
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

    #[test]
    fn split_chunk() {
        const SLICE_SIZE: usize = 10;
        let mut sender = ChunkSender::new(SLICE_SIZE, 100, Duration::from_millis(100), 30);
        let message = Bytes::from(vec![255u8; 30]);
        sender.send_message(message.clone());

        let mut receiver = ChunkReceiver::new(SLICE_SIZE);

        let slice_messages = sender.generate_slice_packets(u64::MAX).unwrap();
        assert_eq!(slice_messages.len(), 2);
        sender.process_ack(0);
        sender.process_ack(1);

        for slice_message in slice_messages.into_iter() {
            receiver.process_slice_message(&slice_message);
        }

        let last_message = sender.generate_slice_packets(u64::MAX).unwrap();
        let result = receiver.process_slice_message(&last_message[0]);
        assert_eq!(message, result.unwrap());
    }

    #[test]
    fn block_chunk() {
        let config = BlockChannelConfig::default();
        let mut sender_channel = BlockChannel::new(config.clone());
        let mut receiver_channel = BlockChannel::new(config);

        let payload = Bytes::from(vec![7u8; 102400]);

        sender_channel.send_message(payload.clone());
        let mut sequence = 0;

        loop {
            let channel_data = sender_channel.get_messages_to_send(1600, sequence);
            match channel_data {
                None => break,
                Some(data) => {
                    receiver_channel.process_messages(data.messages);
                    sender_channel.process_ack(sequence);
                    sequence += 1;
                }
            }
        }

        let received_payload = receiver_channel.receive_message().unwrap();
        assert_eq!(payload.len(), received_payload.len());
    }

    #[test]
    fn block_channel_queue() {
        let mut channel = BlockChannel::new(BlockChannelConfig {
            resend_time: Duration::ZERO,
            ..Default::default()
        });
        let first_message = Bytes::from(vec![3; 2000]);
        let second_message = Bytes::from(vec![5; 2000]);
        channel.send_message(first_message.clone());
        channel.send_message(second_message.clone());

        // First message
        let block_channel_data = channel.get_messages_to_send(u64::MAX, 0).unwrap();
        assert!(!block_channel_data.messages.is_empty());
        channel.process_messages(block_channel_data.messages);
        let received_first_message = channel.receive_message().unwrap();
        assert_eq!(first_message, received_first_message);
        channel.process_ack(0);

        // Second message
        let block_channel_data = channel.get_messages_to_send(u64::MAX, 1).unwrap();
        assert!(!block_channel_data.messages.is_empty());
        channel.process_messages(block_channel_data.messages);
        let received_second_message = channel.receive_message().unwrap();
        assert_eq!(second_message, received_second_message);
        channel.process_ack(1);

        // Check there is no message to send
        assert!(!channel.sender.sending);
        let block_channel_data = channel.get_messages_to_send(u64::MAX, 2);
        assert!(block_channel_data.is_none());
    }

    #[test]
    fn acking_packet_with_old_chunk_id() {
        let mut channel = BlockChannel::new(BlockChannelConfig {
            resend_time: Duration::ZERO,
            ..Default::default()
        });
        let first_message = Bytes::from(vec![5; 400 * 3]);
        let second_message = Bytes::from(vec![3; 400]);
        channel.send_message(first_message);
        channel.send_message(second_message);

        let _ = channel.get_messages_to_send(u64::MAX, 0).unwrap();
        let _ = channel.get_messages_to_send(u64::MAX, 1).unwrap();

        channel.process_ack(0);
        let _ = channel.get_messages_to_send(u64::MAX, 2).unwrap();

        channel.process_ack(1);
        assert!(channel.sender.sending);

        channel.process_ack(2);
        assert!(!channel.sender.sending);
    }
}
