use std::{
    collections::{btree_map, BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use bytes::Bytes;

use super::SliceConstructor;
use crate::{
    error::ChannelError,
    packet::{Packet, Slice, SLICE_SIZE},
};

#[derive(Debug)]
enum UnackedMessage {
    Small {
        message: Bytes,
        last_sent: Option<Duration>,
    },
    Sliced {
        message: Bytes,
        num_slices: usize,
        num_acked_slices: usize,
        next_slice_to_send: usize,
        acked: Vec<bool>,
        last_sent: Vec<Option<Duration>>,
    },
}

#[derive(Debug)]
pub struct SendChannelReliable {
    channel_id: u8,
    unacked_messages: BTreeMap<u64, UnackedMessage>,
    next_reliable_message_id: u64,
    resend_time: Duration,
    max_memory_usage_bytes: usize,
    memory_usage_bytes: usize,
}

#[derive(Debug)]
enum ReliableOrder {
    Ordered,
    Unordered {
        most_recent_message_id: u64,
        received_messages: BTreeSet<u64>,
    },
}

#[derive(Debug)]
pub struct ReceiveChannelReliable {
    slices: HashMap<u64, SliceConstructor>,
    messages: BTreeMap<u64, Bytes>,
    oldest_pending_message_id: u64,
    reliable_order: ReliableOrder,
    memory_usage_bytes: usize,
    max_memory_usage_bytes: usize,
}

impl UnackedMessage {
    fn new_sliced(payload: Bytes) -> Self {
        let num_slices = payload.len().div_ceil(SLICE_SIZE);

        Self::Sliced {
            message: payload,
            num_slices,
            num_acked_slices: 0,
            next_slice_to_send: 0,
            acked: vec![false; num_slices],
            last_sent: vec![None; num_slices],
        }
    }
}

impl SendChannelReliable {
    pub fn new(channel_id: u8, resend_time: Duration, max_memory_usage_bytes: usize) -> Self {
        Self {
            channel_id,
            unacked_messages: BTreeMap::new(),
            next_reliable_message_id: 0,
            resend_time,
            max_memory_usage_bytes,
            memory_usage_bytes: 0,
        }
    }

    pub fn available_memory(&self) -> usize {
        self.max_memory_usage_bytes - self.memory_usage_bytes
    }

    pub fn can_send_message(&self, size_bytes: usize) -> bool {
        size_bytes + self.memory_usage_bytes <= self.max_memory_usage_bytes
    }

    pub fn get_packets_to_send(&mut self, packet_sequence: &mut u64, available_bytes: &mut u64, current_time: Duration) -> Vec<Packet> {
        if self.unacked_messages.is_empty() {
            return vec![];
        }

        let mut packets: Vec<Packet> = vec![];

        let mut small_messages: Vec<(u64, Bytes)> = vec![];
        let mut small_messages_bytes = 0;

        'messages: for (&message_id, unacked_message) in self.unacked_messages.iter_mut() {
            match unacked_message {
                UnackedMessage::Small { message, last_sent } => {
                    if *available_bytes < message.len() as u64 {
                        // Skip message, no bytes available to send this message
                        continue;
                    }

                    if let Some(last_sent) = last_sent {
                        if current_time - *last_sent < self.resend_time {
                            continue;
                        }
                    }

                    *available_bytes -= message.len() as u64;

                    // Generate packet with small messages if you cannot fit
                    let serialized_size = message.len() + octets::varint_len(message.len() as u64) + octets::varint_len(message_id);
                    if small_messages_bytes + serialized_size > SLICE_SIZE {
                        packets.push(Packet::SmallReliable {
                            sequence: *packet_sequence,
                            channel_id: self.channel_id,
                            messages: std::mem::take(&mut small_messages),
                        });
                        small_messages_bytes = 0;
                        *packet_sequence += 1;
                    }

                    small_messages_bytes += serialized_size;
                    small_messages.push((message_id, message.clone()));
                    *last_sent = Some(current_time);

                    continue;
                }
                UnackedMessage::Sliced {
                    message,
                    num_slices,
                    acked,
                    last_sent,
                    next_slice_to_send,
                    ..
                } => {
                    let start_index = *next_slice_to_send;
                    for i in 0..*num_slices {
                        if *available_bytes < SLICE_SIZE as u64 {
                            // Skip message, no bytes available to send a slice
                            continue 'messages;
                        }

                        let i = (start_index + i) % *num_slices;
                        if acked[i] {
                            continue;
                        }

                        if let Some(last_sent) = last_sent[i] {
                            if current_time - last_sent < self.resend_time {
                                continue;
                            }
                        }

                        let start = i * SLICE_SIZE;
                        let end = if i == *num_slices - 1 { message.len() } else { (i + 1) * SLICE_SIZE };

                        let payload = message.slice(start..end);
                        *available_bytes -= payload.len() as u64;

                        let slice = Slice {
                            message_id,
                            slice_index: i,
                            num_slices: *num_slices,
                            payload,
                        };

                        packets.push(Packet::ReliableSlice {
                            sequence: *packet_sequence,
                            channel_id: self.channel_id,
                            slice,
                        });

                        *packet_sequence += 1;
                        last_sent[i] = Some(current_time);
                        *next_slice_to_send = i + 1 % *num_slices;
                    }
                }
            }
        }

        // Generate final packet for remaining small messages
        if !small_messages.is_empty() {
            packets.push(Packet::SmallReliable {
                sequence: *packet_sequence,
                channel_id: self.channel_id,
                messages: std::mem::take(&mut small_messages),
            });
            *packet_sequence += 1;
        }

        packets
    }

    pub fn send_message(&mut self, message: Bytes) -> Result<(), ChannelError> {
        if self.memory_usage_bytes + message.len() > self.max_memory_usage_bytes {
            return Err(ChannelError::ReliableChannelMaxMemoryReached);
        }

        self.memory_usage_bytes += message.len();
        let unacked_message = if message.len() > SLICE_SIZE {
            UnackedMessage::new_sliced(message)
        } else {
            UnackedMessage::Small { message, last_sent: None }
        };

        self.unacked_messages.insert(self.next_reliable_message_id, unacked_message);
        self.next_reliable_message_id += 1;

        Ok(())
    }

    pub fn process_message_ack(&mut self, message_id: u64) {
        if self.unacked_messages.contains_key(&message_id) {
            let unacked_message = self.unacked_messages.remove(&message_id).unwrap();
            let UnackedMessage::Small { message: payload, .. } = unacked_message else {
                unreachable!("called ack on small message but found sliced");
            };
            self.memory_usage_bytes -= payload.len();
        }
    }

    pub fn process_slice_message_ack(&mut self, message_id: u64, slice_index: usize) {
        let Some(unacked_message) = self.unacked_messages.get_mut(&message_id) else {
            return;
        };

        let UnackedMessage::Sliced {
            message,
            num_slices,
            num_acked_slices,
            acked,
            ..
        } = unacked_message
        else {
            unreachable!("called ack on sliced message but found small");
        };

        if acked[slice_index] {
            return;
        }

        acked[slice_index] = true;
        *num_acked_slices += 1;

        if *num_acked_slices == *num_slices {
            self.memory_usage_bytes -= message.len();
            self.unacked_messages.remove(&message_id);
        }
    }
}

impl ReceiveChannelReliable {
    pub fn new(max_memory_usage_bytes: usize, ordered: bool) -> Self {
        let reliable_order = match ordered {
            true => ReliableOrder::Ordered,
            false => ReliableOrder::Unordered {
                most_recent_message_id: 0,
                received_messages: BTreeSet::new(),
            },
        };
        Self {
            slices: HashMap::new(),
            messages: BTreeMap::new(),
            oldest_pending_message_id: 0,
            reliable_order,
            memory_usage_bytes: 0,
            max_memory_usage_bytes,
        }
    }

    pub fn process_message(&mut self, message: Bytes, message_id: u64) -> Result<(), ChannelError> {
        if message_id < self.oldest_pending_message_id {
            // Discard old message already received
            return Ok(());
        }

        match &mut self.reliable_order {
            ReliableOrder::Ordered => {
                if let btree_map::Entry::Vacant(entry) = self.messages.entry(message_id) {
                    if self.memory_usage_bytes + message.len() > self.max_memory_usage_bytes {
                        return Err(ChannelError::ReliableChannelMaxMemoryReached);
                    }
                    self.memory_usage_bytes += message.len();

                    entry.insert(message);
                }
            }
            ReliableOrder::Unordered {
                most_recent_message_id,
                received_messages,
            } => {
                if *most_recent_message_id < message_id {
                    *most_recent_message_id = message_id;
                }

                if !received_messages.contains(&message_id) {
                    if self.memory_usage_bytes + message.len() > self.max_memory_usage_bytes {
                        return Err(ChannelError::ReliableChannelMaxMemoryReached);
                    }
                    self.memory_usage_bytes += message.len();

                    received_messages.insert(message_id);
                    self.messages.insert(message_id, message);
                }
            }
        }

        Ok(())
    }

    pub fn process_slice(&mut self, slice: Slice) -> Result<(), ChannelError> {
        if self.messages.contains_key(&slice.message_id) || slice.message_id < self.oldest_pending_message_id {
            // Message already assembled
            return Ok(());
        }

        if !self.slices.contains_key(&slice.message_id) {
            let message_len = slice.num_slices * SLICE_SIZE;
            if self.memory_usage_bytes + message_len > self.max_memory_usage_bytes {
                return Err(ChannelError::ReliableChannelMaxMemoryReached);
            }
            self.memory_usage_bytes += message_len;
        }

        let slice_constructor = self
            .slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        if let Some(message) = slice_constructor.process_slice(slice.slice_index, &slice.payload)? {
            // Memory usage is re-added with the exactly message size
            self.memory_usage_bytes -= slice.num_slices * SLICE_SIZE;
            self.process_message(message, slice.message_id)?;
            self.slices.remove(&slice.message_id);
        }

        Ok(())
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        match &mut self.reliable_order {
            ReliableOrder::Ordered => {
                let message = self.messages.remove(&self.oldest_pending_message_id)?;

                self.oldest_pending_message_id += 1;
                self.memory_usage_bytes -= message.len();
                Some(message)
            }
            ReliableOrder::Unordered { received_messages, .. } => {
                let (message_id, message) = self.messages.pop_first()?;

                if self.oldest_pending_message_id == message_id {
                    // Remove all next items that could have been received out of order,
                    // until we find an message that was not received
                    while received_messages.contains(&self.oldest_pending_message_id) {
                        received_messages.remove(&self.oldest_pending_message_id);
                        self.oldest_pending_message_id += 1;
                    }
                }

                self.memory_usage_bytes -= message.len();
                Some(message)
            }
        }
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
        let mut current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut recv = ReceiveChannelReliable::new(max_memory, true);
        let mut send = SendChannelReliable::new(0, resend_time, max_memory);

        let message1 = vec![1, 2, 3];
        let message2 = vec![3, 4, 5];

        send.send_message(message1.clone().into()).unwrap();
        send.send_message(message2.clone().into()).unwrap();

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        for packet in packets {
            let Packet::SmallReliable {
                sequence: 0,
                channel_id: 0,
                messages,
            } = packet
            else {
                unreachable!();
            };
            for (message, message_id) in messages {
                recv.process_message(message_id, message).unwrap();
            }
        }

        let new_message1 = recv.receive_message().unwrap();
        let new_message2 = recv.receive_message().unwrap();

        assert_eq!(message1, new_message1);
        assert_eq!(message2, new_message2);

        // Should not resend anything
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert!(packets.is_empty());

        current_time += resend_time;
        // Should resend now
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 1);

        // Should not resend after ack
        current_time += resend_time;
        send.process_message_ack(0);
        send.process_message_ack(1);

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert!(packets.is_empty());
    }

    #[test]
    fn small_packet_unordered() {
        let max_memory: usize = 10000;
        let mut available_bytes = u64::MAX;
        let mut sequence: u64 = 0;
        let mut current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut recv = ReceiveChannelReliable::new(max_memory, false);
        let mut send = SendChannelReliable::new(0, resend_time, max_memory);

        let message1 = vec![1, 2, 3];
        let message2 = vec![3, 4, 5];
        let message3 = vec![6, 7, 8];

        send.send_message(message1.clone().into()).unwrap();
        send.send_message(message2.clone().into()).unwrap();
        send.send_message(message3.clone().into()).unwrap();

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 1);
        let Packet::SmallReliable { messages, .. } = &packets[0] else {
            unreachable!();
        };

        assert_eq!(messages.len(), 3);

        // Process and receive out of order
        recv.process_message(messages[2].1.clone(), messages[2].0).unwrap();
        let new_message3 = recv.receive_message().unwrap();

        recv.process_message(messages[1].1.clone(), messages[1].0).unwrap();
        let new_message2 = recv.receive_message().unwrap();

        recv.process_message(messages[0].1.clone(), messages[0].0).unwrap();
        let new_message1 = recv.receive_message().unwrap();

        assert_eq!(message1, new_message1);
        assert_eq!(message2, new_message2);
        assert_eq!(message3, new_message3);

        match &recv.reliable_order {
            ReliableOrder::Ordered => unreachable!(),
            ReliableOrder::Unordered {
                most_recent_message_id,
                received_messages,
            } => {
                assert_eq!(*most_recent_message_id, 2);
                assert!(received_messages.is_empty());
            }
        }

        // Should not resend anything
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert!(packets.is_empty());

        current_time += resend_time;
        // Should resend now
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 1);

        // Should not resend after ack
        current_time += resend_time;
        send.process_message_ack(0);
        send.process_message_ack(1);
        send.process_message_ack(2);

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert!(packets.is_empty());
    }

    #[test]
    fn slice_packet() {
        let max_memory: usize = 10000;
        let mut available_bytes = u64::MAX;
        let mut sequence: u64 = 0;
        let mut current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut recv = ReceiveChannelReliable::new(max_memory, true);
        let mut send = SendChannelReliable::new(0, resend_time, max_memory);

        let message = vec![5; SLICE_SIZE * 3];

        send.send_message(message.clone().into()).unwrap();

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        for packet in packets {
            let Packet::ReliableSlice { channel_id: 0, slice, .. } = packet else {
                unreachable!();
            };
            recv.process_slice(slice).unwrap();
        }

        let new_message = recv.receive_message().unwrap();
        assert_eq!(message, new_message);

        // Should not resend anything
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert!(packets.is_empty());

        current_time += resend_time;
        // Should resend now
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 3);

        // Should not resend after ack
        current_time += resend_time;
        send.process_slice_message_ack(0, 0);
        send.process_slice_message_ack(0, 1);
        send.process_slice_message_ack(0, 2);

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert!(packets.is_empty());
    }

    #[test]
    fn max_memory() {
        let mut available_bytes = u64::MAX;
        let mut sequence: u64 = 0;
        let current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut recv = ReceiveChannelReliable::new(99, true);
        let mut send = SendChannelReliable::new(0, resend_time, 101);

        let message = vec![5; 100];

        // Can send one message without reaching memory limit
        send.send_message(message.clone().into()).unwrap();

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        for packet in packets {
            let Packet::SmallReliable {
                sequence: 0,
                channel_id: 0,
                messages,
            } = packet
            else {
                unreachable!();
            };
            for (message, message_id) in messages {
                let Err(e) = recv.process_message(message_id, message) else {
                    unreachable!();
                };
                assert_eq!(e, ChannelError::ReliableChannelMaxMemoryReached);
            }
        }

        let Err(send_err) = send.send_message(message.into()) else {
            unreachable!()
        };
        assert_eq!(send_err, ChannelError::ReliableChannelMaxMemoryReached);
    }

    #[test]
    fn available_bytes() {
        let mut sequence: u64 = 0;
        let current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut send = SendChannelReliable::new(0, resend_time, usize::MAX);

        let message: Bytes = vec![0u8; 100].into();
        send.send_message(message.clone()).unwrap();
        send.send_message(message).unwrap();

        // No available bytes
        let mut available_bytes: u64 = 50;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 0);

        // Bytes for 1 message
        let mut available_bytes: u64 = 100;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 1);

        // Bytes for 1 message
        let mut available_bytes: u64 = 100;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 1);

        // No more messages to send
        let mut available_bytes: u64 = u64::MAX;
        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 0);
    }

    #[test]
    fn small_packet_max_size() {
        let mut sequence: u64 = 0;
        let current_time: Duration = Duration::ZERO;
        let mut available_bytes = u64::MAX;
        let resend_time = Duration::from_millis(100);
        let mut send = SendChannelReliable::new(0, resend_time, usize::MAX);

        // 4 bytes
        let message: Bytes = vec![0, 1, 2, 3].into();

        // (4 + 1 + 2) * 300 = 2100 = 2 packets
        for _ in 0..300 {
            send.send_message(message.clone()).unwrap();
        }

        let packets = send.get_packets_to_send(&mut sequence, &mut available_bytes, current_time);
        assert_eq!(packets.len(), 2);
        let mut buffer = [0u8; 1400];
        for packet in packets {
            let mut oct = OctetsMut::with_slice(&mut buffer);
            let len = packet.to_bytes(&mut oct).unwrap();
            assert!(len < 1300);
        }
    }
}
