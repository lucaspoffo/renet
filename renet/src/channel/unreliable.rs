use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    channel::SliceConstructor,
    error::ChannelError,
    packet::{Packet, Slice, SLICE_SIZE},
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
    slices: BTreeMap<u64, SliceConstructor>,
    slices_last_received: BTreeMap<u64, Duration>,
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
            self.memory_usage_bytes -= message.len();
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
                        sequence: *packet_sequence,
                        channel_id: self.channel_id,
                        slice,
                    });
                    *packet_sequence += 1;
                }

                self.sliced_message_id += 1;
            } else {
                let serialized_size = message.len() + octets::varint_len(message.len() as u64);
                if small_messages_bytes + serialized_size > SLICE_SIZE {
                    packets.push(Packet::SmallUnreliable {
                        sequence: *packet_sequence,
                        channel_id: self.channel_id,
                        messages: std::mem::take(&mut small_messages),
                    });
                    *packet_sequence += 1;
                    small_messages_bytes = 0;
                }

                small_messages_bytes += serialized_size;
                small_messages.push(message);
            }
        }

        // Generate final packet for remaining small messages
        if !small_messages.is_empty() {
            packets.push(Packet::SmallUnreliable {
                sequence: *packet_sequence,
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
            slices: BTreeMap::new(),
            slices_last_received: BTreeMap::new(),
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
        self.messages.push_back(message);
    }

    pub fn process_slice(&mut self, slice: Slice, current_time: Duration) -> Result<(), ChannelError> {
        if !self.slices.contains_key(&slice.message_id) {
            let message_len = slice.num_slices * SLICE_SIZE;
            if self.max_memory_usage_bytes < self.memory_usage_bytes + message_len {
                log::warn!(
                    "dropped unreliable slice message received because channel {} is memory limited",
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

    pub fn discard_incomplete_old_slices(&mut self, current_time: Duration) {
        let mut lost_messages: Vec<u64> = Vec::new();
        for (&message_id, last_received) in self.slices_last_received.iter() {
            const DISCARD_AFTER: Duration = Duration::from_secs(3);
            if current_time - *last_received >= DISCARD_AFTER {
                lost_messages.push(message_id);
            } else {
                // If the current message is not discard, the next ones will not be discarded
                // since all the next message were sent after this one.
                break;
            }
        }

        for message_id in lost_messages.iter() {
            self.slices_last_received.remove(message_id);
            let slice = self.slices.remove(message_id).expect("discarded slice should exist");
            self.memory_usage_bytes -= slice.num_slices * SLICE_SIZE;
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        if let Some(message) = self.messages.pop_front() {
            self.memory_usage_bytes -= message.len();
            return Some(message);
        };

        None
    }

    /// Returns the last message received on the channel.
    /// Discard all the previous messages.
    pub fn receive_last_message(&mut self) -> Option<Bytes> {
        while self.messages.len() > 1 {
            let message = self.messages.pop_front().unwrap();
            self.memory_usage_bytes -= message.len();
        }
        if let Some(message) = self.messages.pop_front() {
            self.memory_usage_bytes -= message.len();
            return Some(message);
        };
        None
    }
}

#[cfg(test)]
mod tests {
    use octets::OctetsMut;

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
        send.send_message(message.into());

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
        let mut send = SendChannelUnreliable::new(0, usize::MAX);

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
        send.send_message(message);

        // Space for 1 message
        let mut available_bytes: u64 = 100;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert_eq!(packets.len(), 1);

        // Second message was dropped
        let mut available_bytes: u64 = u64::MAX;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert_eq!(packets.len(), 0);
    }

    #[test]
    fn small_packet_max_size() {
        let mut sequence: u64 = 0;
        let mut available_bytes = u64::MAX;
        let mut send = SendChannelUnreliable::new(0, usize::MAX);

        // 4 bytes
        let message: Bytes = vec![0, 1, 2, 3].into();

        // (4 + 1) * 400 = 2000 = 2 packets
        for _ in 0..400 {
            send.send_message(message.clone());
        }

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert_eq!(packets.len(), 2);
        let mut buffer = [0u8; 1400];
        for packet in packets {
            let mut oct = OctetsMut::with_slice(&mut buffer);
            let len = packet.to_bytes(&mut oct).unwrap();
            assert!(len < 1300);
        }
    }

    #[test]
    fn receive_last_message_unreliable() {
        let max_memory: usize = 1000;
        let mut available_bytes = u64::MAX;
        let mut sequence: u64 = 0;
        let mut recv = ReceiveChannelUnreliable::new(0, max_memory);
        let mut send = SendChannelUnreliable::new(0, max_memory);

        let message1 = vec![1, 2, 3];
        let message2 = vec![3, 4, 5];
        let message3 = vec![6, 7, 8];

        send.send_message(message1.clone().into());
        send.send_message(message2.clone().into());
        send.send_message(message3.clone().into());

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
        let new_message2 = recv.receive_last_message().unwrap();
        assert!(recv.receive_message().is_none());

        assert_eq!(message1, new_message1);
        assert_eq!(message3, new_message2);

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes);
        assert!(packets.is_empty());
    }
}
