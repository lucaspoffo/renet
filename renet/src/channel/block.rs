use std::{collections::VecDeque, mem, time::Duration};

use bincode::Options;
use serde::{Deserialize, Serialize};

use crate::{error::RenetError, packet::Payload, sequence_buffer::SequenceBuffer, timer::Timer};
use log::{error, info};

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
    slice_ids: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct BlockChannelConfig {
    pub slice_size: usize,
    pub resend_time: Duration,
    pub sent_packet_buffer_size: usize,
    pub packet_budget: u64,
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
    chunk_data: Payload,
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
    sender: ChunkSender,
    receiver: ChunkReceiver,
    received_messages: VecDeque<Payload>,
}

impl Default for BlockChannelConfig {
    fn default() -> Self {
        Self {
            slice_size: 400,
            resend_time: Duration::from_millis(300),
            sent_packet_buffer_size: 256,
            packet_budget: 2000,
        }
    }
}

impl PacketSent {
    fn new(slice_ids: Vec<u32>) -> Self {
        Self {
            acked: false,
            slice_ids,
        }
    }
}

impl ChunkSender {
    fn new(
        slice_size: usize,
        sent_packet_buffer_size: usize,
        resend_time: Duration,
        packet_budget: u64,
    ) -> Self {
        Self {
            sending: false,
            chunk_id: 0,
            slice_size,
            num_slices: 0,
            current_slice_id: 0,
            num_acked_slices: 0,
            acked: Vec::new(),
            chunk_data: Vec::new(),
            resend_timers: Vec::with_capacity(sent_packet_buffer_size),
            packets_sent: SequenceBuffer::with_capacity(sent_packet_buffer_size),
            resend_time,
            packet_budget,
        }
    }

    fn send_message(&mut self, data: Payload) -> Result<(), RenetError> {
        if self.sending {
            return Err(RenetError::AlreadySendingBlockMessage);
        }

        self.sending = true;
        self.num_slices = (data.len() + self.slice_size - 1) / self.slice_size;

        self.acked = vec![false; self.num_slices];
        let mut resend_timer = Timer::new(self.resend_time);
        resend_timer.finish();
        self.resend_timers.clear();
        self.resend_timers.resize(self.num_slices, resend_timer);
        self.chunk_data = data;
        Ok(())
    }

    fn generate_slice_packets(
        &mut self,
        mut available_bytes: u64,
    ) -> Result<Vec<SliceMessage>, RenetError> {
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
            let end = if slice_id == self.num_slices - 1 {
                self.chunk_data.len()
            } else {
                (slice_id + 1) * self.slice_size
            };

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
            if sent_packet.acked {
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
            return Some(block);
        }

        None
    }
}

impl BlockChannel {
    pub fn new(config: BlockChannelConfig) -> Self {
        let sender = ChunkSender::new(
            config.slice_size,
            config.sent_packet_buffer_size,
            config.resend_time,
            config.packet_budget,
        );
        let receiver = ChunkReceiver::new(config.slice_size);

        Self {
            sender,
            receiver,
            received_messages: VecDeque::new(),
        }
    }

    pub fn advance_time(&mut self, duration: Duration) {
        for timer in self.sender.resend_timers.iter_mut() {
            timer.advance(duration);
        }
    }

    pub fn get_messages_to_send(
        &mut self,
        available_bytes: u64,
        sequence: u16,
    ) -> Result<Vec<SliceMessage>, RenetError> {
        let messages: Vec<SliceMessage> = self.sender.generate_slice_packets(available_bytes)?;

        if messages.is_empty() {
            return Ok(messages);
        }

        let slice_ids: Vec<u32> = messages.iter().map(|m| m.slice_id).collect();

        let packet_sent = PacketSent::new(slice_ids);
        self.sender.packets_sent.insert(sequence, packet_sent);

        Ok(messages)
    }

    pub fn process_slice_messages(&mut self, messages: Vec<SliceMessage>) {
        for message in messages.iter() {
            if let Some(block) = self.receiver.process_slice_message(message) {
                self.received_messages.push_back(block);
            }
        }
    }

    pub fn process_ack(&mut self, ack: u16) {
        self.sender.process_ack(ack);
    }

    pub fn send_message(&mut self, message_payload: Payload) -> Result<(), RenetError> {
        self.sender.send_message(message_payload)
    }

    pub fn receive_message(&mut self) -> Option<Payload> {
        self.received_messages.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_chunk() {
        const SLICE_SIZE: usize = 10;
        let mut sender = ChunkSender::new(SLICE_SIZE, 100, Duration::from_millis(100), 30);
        let message = vec![255u8; 30];
        sender.send_message(message.clone()).unwrap();

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

        let payload = vec![7u8; 102400];

        sender_channel.send_message(payload.clone()).unwrap();
        let mut sequence = 0;

        loop {
            let messages = sender_channel.get_messages_to_send(1600, sequence).unwrap();
            if messages.is_empty() {
                break;
            }
            receiver_channel.process_slice_messages(messages);
            sender_channel.process_ack(sequence);
            sequence += 1;
        }

        let received_payload = receiver_channel.receive_message().unwrap();
        assert_eq!(payload.len(), received_payload.len());
    }
}
