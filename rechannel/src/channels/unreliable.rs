use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use bytes::Bytes;

use super::{SliceConstructor, SLICE_SIZE};
use crate::{
    error::ChannelError,
    packet::{Packet, Slice},
};

#[derive(Debug)]
pub struct SendChannelUnreliable {
    channel_id: u8,
    unreliable_messages: VecDeque<Bytes>,
    sliced_message_id: u64,
    max_memory_usage_bytes: usize,
    memory_usage_bytes: usize,
}

#[derive(Debug)]
pub struct ReceiveChannelUnreliable {
    channel_id: u8,
    messages: VecDeque<Bytes>,
    slices: HashMap<u64, SliceConstructor>,
    slices_last_received: HashMap<u64, Duration>,
    max_memory_usage_bytes: usize,
    memory_usage_bytes: usize,
}

impl SendChannelUnreliable {
    pub fn new(channel_id: u8, max_memory_usage_bytes: usize) -> Self {
        Self {
            channel_id,
            unreliable_messages: VecDeque::new(),
            sliced_message_id: 0,
            max_memory_usage_bytes,
            memory_usage_bytes: 0,
        }
    }

    pub fn get_packets_to_send(&mut self, packet_sequence: &mut u64, available_bytes: &mut u64) -> Vec<Packet> {
        let mut packets: Vec<Packet> = vec![];
        let mut small_messages: Vec<Bytes> = vec![];
        let mut small_messages_bytes = 0;

        while let Some(message) = self.unreliable_messages.pop_front() {
            if *available_bytes < message.len() as u64 {
                // Drop message, no available bytes to send
                continue;
            }

            *available_bytes -= message.len() as u64;
            if message.len() > SLICE_SIZE {
                let num_slices = (message.len() + SLICE_SIZE - 1) / SLICE_SIZE;

                for slice_index in 0..num_slices {
                    let start = slice_index * SLICE_SIZE;
                    let end = if slice_index == num_slices - 1 { message.len() } else { (slice_index + 1) * SLICE_SIZE };
                    let payload = message.slice(start..end);

                    let slice = Slice {
                        message_id: self.sliced_message_id,
                        slice_index,
                        num_slices,
                        payload,
                    };

                    packets.push(Packet::UnreliableSlice {
                        packet_sequence: *packet_sequence,
                        channel_id: self.channel_id,
                        slice,
                    });
                    *packet_sequence += 1;
                }

                self.sliced_message_id += 1;
            } else {
                if small_messages_bytes + message.len() > SLICE_SIZE {
                    packets.push(Packet::SmallUnreliable {
                        packet_sequence: *packet_sequence,
                        channel_id: self.channel_id,
                        messages: std::mem::take(&mut small_messages),
                    });
                    *packet_sequence += 1;
                    small_messages_bytes = 0;
                }

                small_messages_bytes += message.len();
                small_messages.push(message);
            }
        }

        // Generate final packet for remaining small messages
        if !small_messages.is_empty() {
            packets.push(Packet::SmallUnreliable {
                packet_sequence: *packet_sequence,
                channel_id: self.channel_id,
                messages: std::mem::take(&mut small_messages),
            });
            *packet_sequence += 1;
        }

        packets
    }

    pub fn send_message(&mut self, message: Bytes) {
        if self.max_memory_usage_bytes < self.memory_usage_bytes + message.len() {
            log::warn!(
                "dropped unreliable message sent because channel {} is memory limited",
                self.channel_id
            );
            return;
        }

        self.memory_usage_bytes += message.len();
        self.unreliable_messages.push_back(message);
    }
}

impl ReceiveChannelUnreliable {
    pub fn new(channel_id: u8, max_memory_usage_bytes: usize) -> Self {
        Self {
            channel_id,
            slices: HashMap::new(),
            slices_last_received: HashMap::new(),
            messages: VecDeque::new(),
            memory_usage_bytes: 0,
            max_memory_usage_bytes,
        }
    }

    pub fn process_message(&mut self, message: Bytes) {
        if self.max_memory_usage_bytes < self.memory_usage_bytes + message.len() {
            log::warn!(
                "dropped unreliable message received because channel {} is memory limited",
                self.channel_id
            );
            return;
        }

        self.memory_usage_bytes += message.len();
        self.messages.push_back(message.into());
    }

    pub fn process_slice(&mut self, slice: Slice, current_time: Duration) -> Result<(), ChannelError> {
        if !self.slices.contains_key(&slice.message_id) {
            let message_len = slice.num_slices * SLICE_SIZE;
            if self.max_memory_usage_bytes < self.memory_usage_bytes + message_len {
                log::warn!(
                    "dropped unreliable message sent because channel {} is memory limited",
                    self.channel_id
                );
                return Ok(());
            }

            self.memory_usage_bytes += message_len;
        }

        let slice_constructor = self
            .slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        if let Some(message) = slice_constructor.process_slice(slice.slice_index, &slice.payload)? {
            self.slices.remove(&slice.message_id);
            self.slices_last_received.remove(&slice.message_id);
            self.memory_usage_bytes -= slice.num_slices * SLICE_SIZE;
            self.memory_usage_bytes += message.len();
            self.messages.push_back(message);
        } else {
            self.slices_last_received.insert(slice.message_id, current_time);
        }

        Ok(())
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        let Some(message) = self.messages.pop_front() else {
            return None
        };
        self.memory_usage_bytes -= message.len();
        Some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_packet() {
        let max_memory: usize = 10000;
        let mut available_bytes = u64::MAX;
        let mut sequence: u64 = 0;
        let mut recv = ReceiveChannelUnreliable::new(0, max_memory);
        let mut send = SendChannelUnreliable::new(0, max_memory);

        let message1 = vec![1, 2, 3];
        let message2 = vec![3, 4, 5];

        send.send_message(message1.clone().into());
        send.send_message(message2.clone().into());

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        for packet in packets {
            let Packet::SmallUnreliable { messages, .. } = packet else {
                unreachable!();
            };
            for message in messages {
                recv.process_message(message);
            }
        }

        let new_message1 = recv.receive_message().unwrap();
        let new_message2 = recv.receive_message().unwrap();
        assert!(recv.receive_message().is_none());

        assert_eq!(message1, new_message1);
        assert_eq!(message2, new_message2);

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert!(packets.is_empty());
    }

    #[test]
    fn slice_packet() {
        let max_memory: usize = 10000;
        let mut available_bytes = u64::MAX;
        let mut sequence: u64 = 0;
        let current_time = Duration::ZERO;
        let mut recv = ReceiveChannelUnreliable::new(0, max_memory);
        let mut send = SendChannelUnreliable::new(0, max_memory);

        let message = vec![5; SLICE_SIZE * 3];

        send.send_message(message.clone().into());

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        for packet in packets {
            let Packet::UnreliableSlice { slice, .. } = packet else {
                unreachable!();
            };
            recv.process_slice(slice, current_time).unwrap();
        }

        let new_message = recv.receive_message().unwrap();
        assert!(recv.receive_message().is_none());

        assert_eq!(message, new_message);

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert!(packets.is_empty());
    }

    #[test]
    fn max_memory() {
        let mut sequence: u64 = 0;
        let mut available_bytes = u64::MAX;
        let mut recv = ReceiveChannelUnreliable::new(0, 50);
        let mut send = SendChannelUnreliable::new(0, 40);

        let message = vec![5; 50];

        send.send_message(message.clone().into());
        send.send_message(message.clone().into());

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        for packet in packets {
            let Packet::SmallUnreliable { messages, .. } = packet else {
                unreachable!();
            };

            // Second message was dropped
            assert_eq!(messages.len(), 1);
            for message in messages {
                recv.process_message(message);
            }
        }

        // The processed message was dropped because there was no memory available
        assert!(recv.receive_message().is_none());
    }

    #[test]
    fn available_bytes() {
        let mut sequence: u64 = 0;
        let mut send = SendChannelUnreliable::new(0,  usize::MAX);

        let message: Bytes = vec![0u8; 100].into();
        send.send_message(message.clone());

        // No available bytes
        let mut available_bytes: u64 = 50;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert_eq!(packets.len(), 0);

        // Available space but message was dropped
        let mut available_bytes: u64 = u64::MAX;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert_eq!(packets.len(), 0);

        send.send_message(message.clone());
        send.send_message(message.clone());

        // Space for 1 message
        let mut available_bytes: u64 = 100;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert_eq!(packets.len(), 1);

        // Second message was dropped
        let mut available_bytes: u64 = u64::MAX;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert_eq!(packets.len(), 0);
    }
}
