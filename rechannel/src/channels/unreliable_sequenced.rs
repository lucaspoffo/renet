use std::{
    collections::{BTreeMap, VecDeque, HashMap},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    another_channel::{Packet, Slice},
    error::ChannelError,
};

use super::{SLICE_SIZE, SliceConstructor};

pub struct SendChannelUnreliableSequenced {
    channel_id: u8,
    message_id: u64,
    unreliable_messages: VecDeque<Bytes>,
    max_memory_usage_bytes: usize,
    memory_usage_bytes: usize,
    error: Option<ChannelError>,
}


pub struct ReceiveChannelUnreliableSequenced {
    messages: BTreeMap<u64, Bytes>,
    slices: HashMap<u64, SliceConstructor>,
    slices_last_received: HashMap<u64, Duration>,
    next_message_id: u64,
    max_memory_usage_bytes: usize,
    memory_usage_bytes: usize,
    error: Option<ChannelError>,
}

impl SendChannelUnreliableSequenced {
    pub fn new(channel_id: u8, max_memory_usage_bytes: usize) -> Self {
        Self {
            channel_id,
            message_id: 0,
            unreliable_messages: VecDeque::new(),
            max_memory_usage_bytes,
            memory_usage_bytes: 0,
            error: None,
        }
    }
    fn get_messages_to_send(&mut self, packet_sequence: &mut u64) -> Vec<Packet> {
        let mut packets: Vec<Packet> = vec![];

        let mut small_messages: Vec<(u64, Bytes)> = vec![];
        let mut small_messages_bytes = 0;

        let generate_normal_packet = |messages: &mut Vec<(u64, Bytes)>, packets: &mut Vec<Packet>, packet_sequence: u64| {
            let messages = std::mem::take(messages);

            packets.push(Packet::SmallUnreliableSequenced {
                packet_sequence,
                channel_id: self.channel_id,
                messages,
            });
        };

        while let Some(unreliable_message) = self.unreliable_messages.pop_front() {
            if unreliable_message.len() > SLICE_SIZE {
                let num_slices = unreliable_message.len() / SLICE_SIZE;
                // TODO: refactor this (used in multiple places)
                for slice_index in 0..num_slices {
                    let start = slice_index * SLICE_SIZE;
                    let end = if slice_index == num_slices - 1 { unreliable_message.len() } else { (slice_index + 1) * SLICE_SIZE };
                    let payload = unreliable_message.slice(start..end);

                    let slice = Slice {
                        message_id: self.message_id,
                        slice_index,
                        num_slices,
                        payload,
                    };

                    packets.push(Packet::MessageSlice {
                        packet_sequence: *packet_sequence,
                        channel_id: self.channel_id,
                        slice,
                    });

                    *packet_sequence += 1;
                }

                self.message_id += 1;
            } else {
                // FIXME: use const
                if small_messages_bytes + unreliable_message.len() > 1200 {
                    generate_normal_packet(&mut small_messages, &mut packets, *packet_sequence);
                    *packet_sequence += 1;
                    small_messages_bytes = 0;
                }

                small_messages_bytes += unreliable_message.len();
                small_messages.push((self.message_id, unreliable_message));
                self.message_id += 1;
            }
        }

        // Generate final packet for remaining small messages
        if !small_messages.is_empty() {
            generate_normal_packet(&mut small_messages, &mut packets, *packet_sequence);
            *packet_sequence += 1;
        }

        self.memory_usage_bytes = 0;

        packets
    }

    fn send_message(&mut self, message: Bytes) {
        if self.max_memory_usage_bytes < self.memory_usage_bytes + message.len() {
            // TODO: log::warm
            return;
        }

        self.memory_usage_bytes += message.len();
        self.unreliable_messages.push_back(message);
    }
}


impl ReceiveChannelUnreliableSequenced {
    fn new(max_memory_usage_bytes: usize) -> Self {
        Self {
            slices: HashMap::new(),
            slices_last_received: HashMap::new(),
            messages: BTreeMap::new(),
            next_message_id: 0,
            memory_usage_bytes: 0,
            max_memory_usage_bytes,
            error: None,
        }
    }

    pub fn process_message(&mut self, message: Bytes, message_id: u64) {
        if message_id < self.next_message_id {
            // Drop old message
            return;
        }

        if self.max_memory_usage_bytes < self.memory_usage_bytes + message.len() {
            // FIXME: log::warn dropped message
            return;
        }

        self.memory_usage_bytes += message.len();
        self.messages.insert(message_id, message.into());
    }

    pub fn process_slice(&mut self, slice: Slice, current_time: Duration) {
        if slice.message_id < self.next_message_id {
            // Drop old message
            return;
        }

        if !self.slices.contains_key(&slice.message_id) {
            let message_len = slice.num_slices * SLICE_SIZE;
            if self.max_memory_usage_bytes < self.memory_usage_bytes + message_len {
                // FIXME: log::warn dropped message
                return;
            }
            
            self.memory_usage_bytes += message_len;
        }

        let slice_constructor = self
            .slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.slices.remove(&slice.message_id);
                self.slices_last_received.remove(&slice.message_id);
                self.memory_usage_bytes -= slice.num_slices * SLICE_SIZE;
                self.messages.insert(slice.message_id, message);
            }
            Ok(None) => {
                self.slices_last_received.insert(slice.message_id, current_time);
            }
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        let Some((message_id, message)) = self.messages.pop_first() else {
            return None;    
        };

        // Remove any old slices still being assembled 
        for i in self.next_message_id..message_id {
            self.slices.remove(&i);
            self.slices_last_received.remove(&i);
        }

        self.next_message_id = message_id + 1;
        Some(message)
    }
}

mod tests {
    use crate::packet;

    use super::*;

    #[test]
    fn small_packet() {
        let max_memory: usize = 10000;
        let mut sequence: u64 = 0;
        let mut recv = ReceiveChannelUnreliableSequenced::new(max_memory);
        let mut send = SendChannelUnreliableSequenced::new(0, max_memory);

        let message1 = vec![1, 2, 3];
        let message2 = vec![3, 4, 5];

        send.send_message(message1.clone().into());
        send.send_message(message2.clone().into());

        let packets = send.get_messages_to_send(&mut sequence);
        for packet in packets {
            let Packet::SmallUnreliableSequenced { packet_sequence: 0, channel_id: 0, messages } = packet else {
                unreachable!();
            };
            for (message_id, message) in messages {
                recv.process_message(message, message_id);
            }
        }

        let new_message1 = recv.receive_message().unwrap();
        let new_message2 = recv.receive_message().unwrap();
        assert!(recv.receive_message().is_none());

        assert_eq!(message1, new_message1);
        assert_eq!(message2, new_message2);

        let packets = send.get_messages_to_send(&mut sequence);
        assert!(packets.is_empty());
    }

    #[test]
    fn small_packet_out_of_order() {
        let max_memory: usize = 10000;
        let mut sequence: u64 = 0;
        let mut recv = ReceiveChannelUnreliableSequenced::new(max_memory);
        let mut send = SendChannelUnreliableSequenced::new(0, max_memory);

        let message1 = vec![1, 2, 3];
        let message2 = vec![3, 4, 5];

        send.send_message(message1.clone().into());
        send.send_message(message2.clone().into());

        let packets = send.get_messages_to_send(&mut sequence);
        let Packet::SmallUnreliableSequenced { messages, .. } =  &packets[0] else {
            unreachable!()
        };
        
        // Process second message
        recv.process_message(messages[1].1.clone(), messages[1].0);

        let new_message2 = recv.receive_message().unwrap();
        assert_eq!(message2, new_message2);

        // Process first message
        recv.process_message(messages[0].1.clone(), messages[0].0);
        assert!(recv.receive_message().is_none());
    }

    #[test]
    fn slice_packet() {
        let max_memory: usize = 10000;
        let mut sequence: u64 = 0;
        let current_time = Duration::ZERO;
        let mut recv = ReceiveChannelUnreliableSequenced::new(max_memory);
        let mut send = SendChannelUnreliableSequenced::new(0, max_memory);

        let message = vec![5; SLICE_SIZE * 3];

        send.send_message(message.clone().into());

        let packets = send.get_messages_to_send(&mut sequence);
        for packet in packets {
            let Packet::MessageSlice { channel_id: 0, slice, .. } = packet else {
                unreachable!();
            };
            recv.process_slice(slice, current_time);
        }

        let new_message = recv.receive_message().unwrap();
        assert!(recv.receive_message().is_none());

        assert_eq!(message, new_message);

        let packets = send.get_messages_to_send(&mut sequence);
        assert!(packets.is_empty());
    }

    #[test]
    fn max_memory() {
        let mut sequence: u64 = 0;
        let mut recv = ReceiveChannelUnreliableSequenced::new(50);
        let mut send = SendChannelUnreliableSequenced::new(0, 40);

        let message = vec![5; 50];

        send.send_message(message.clone().into());
        send.send_message(message.clone().into());

        let packets = send.get_messages_to_send(&mut sequence);
        for packet in packets {
            let Packet::SmallUnreliableSequenced { packet_sequence: 0, channel_id: 0, messages } = packet else {
                unreachable!();
            };

            // Second message was dropped
            assert_eq!(messages.len(), 1);

            for (message_id, message) in messages {
                recv.process_message(message, message_id);
            }
        }

        // The processed message was dropped because there was no memory available
        assert!(recv.receive_message().is_none());
    }
}
