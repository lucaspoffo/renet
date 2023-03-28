use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    another_channel::{Packet, Slice},
    error::ChannelError,
};

use super::{ChannelConfig, SliceConstructor, SLICE_SIZE};

enum UnackedMessage {
    Small {
        payload: Bytes,
        last_sent: Option<Duration>,
    },
    Sliced {
        payload: Bytes,
        num_slices: usize,
        num_acked_slices: usize,
        acked: Vec<bool>,
        last_sent: Vec<Option<Duration>>,
    },
}

pub struct SendChannelReliable {
    channel_id: u8,
    unacked_messages: BTreeMap<u64, UnackedMessage>,
    next_reliable_message_id: u64,
    oldest_unacked_message_id: u64,
    resend_time: Duration,
    max_memory_usage_bytes: usize,
    memory_usage_bytes: usize,
    error: Option<ChannelError>,
}

enum ReliableOrder {
    Ordered,
    Unordered {
        most_recent_message_id: u64,
        received_messages: BTreeSet<u64>,
    },
}

pub struct ReceiveChannelReliable {
    slices: HashMap<u64, SliceConstructor>,
    messages: BTreeMap<u64, Bytes>,
    next_message_id: u64,
    reliable_order: ReliableOrder,
    memory_usage_bytes: usize,
    max_memory_usage_bytes: usize,
    error: Option<ChannelError>,
}

impl UnackedMessage {
    fn new_sliced(payload: Bytes) -> Self {
        let num_slices = payload.len() / SLICE_SIZE;

        Self::Sliced {
            payload,
            num_slices,
            num_acked_slices: 0,
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
            oldest_unacked_message_id: 0,
            resend_time,
            max_memory_usage_bytes,
            memory_usage_bytes: 0,
            error: None,
        }
    }

    pub fn get_messages_to_send(&mut self, packet_sequence: &mut u64, current_time: Duration) -> Vec<Packet> {
        let mut packets: Vec<Packet> = vec![];

        let mut normal_messages: Vec<(u64, Bytes)> = vec![];
        let mut normal_messages_bytes = 0;

        let mut generate_normal_packet = |messages: &mut Vec<(u64, Bytes)>, packets: &mut Vec<Packet>, packet_sequence: &mut u64| {
            let messages = std::mem::take(messages);

            packets.push(Packet::SmallReliable {
                packet_sequence: packet_sequence.clone(),
                channel_id: self.channel_id,
                messages,
            });
            *packet_sequence += 1;
            normal_messages_bytes = 0;
        };

        let should_send = |last_sent: Option<Duration>| {
            if let Some(last_sent) = last_sent {
                return current_time - last_sent >= self.resend_time;
            }
            true
        };

        for (&message_id, unacked_message) in self.unacked_messages.iter_mut() {
            match unacked_message {
                UnackedMessage::Small { payload, last_sent } => {
                    if !should_send(*last_sent) {
                        continue;
                    }

                    // Generate packet with small messages if you cannot fit
                    // if normal_messages_bytes + message.serialized_size() > MAX_PACKET_SIZE {
                    // generate_normal_packet();
                    // }

                    // normal_messages_bytes += message.serilized_size();
                    normal_messages.push((message_id, payload.clone()));
                    *last_sent = Some(current_time);

                    continue;
                }
                UnackedMessage::Sliced {
                    payload,
                    num_slices,
                    acked,
                    last_sent,
                    ..
                } => {
                    for i in 0..*num_slices {
                        if acked[i] {
                            continue;
                        }

                        if !should_send(last_sent[i]) {
                            continue;
                        }

                        let start = i * SLICE_SIZE;
                        let end = if i == *num_slices - 1 { payload.len() } else { (i + 1) * SLICE_SIZE };

                        let payload = payload.slice(start..end);

                        let slice = Slice {
                            message_id,
                            slice_index: i,
                            num_slices: *num_slices,
                            payload,
                        };

                        packets.push(Packet::MessageSlice {
                            packet_sequence: *packet_sequence,
                            channel_id: self.channel_id,
                            slice,
                        });

                        *packet_sequence += 1;
                        last_sent[i] = Some(current_time);
                    }
                }
            }
        }

        // Generate final packet for remaining small messages
        if !normal_messages.is_empty() {
            generate_normal_packet(&mut normal_messages, &mut packets, packet_sequence);
        }

        packets
    }

    pub fn send_message(&mut self, message: Bytes) {
        if self.memory_usage_bytes + message.len() > self.max_memory_usage_bytes {
            // FIXME: use correct error ::MaxMemoryReached
            self.error = Some(ChannelError::SendQueueFull);
            return;
        }

        self.memory_usage_bytes += message.len();
        let unacked_message = if message.len() > SLICE_SIZE {
            UnackedMessage::new_sliced(message)
        } else {
            UnackedMessage::Small {
                payload: message,
                last_sent: None,
            }
        };

        self.unacked_messages.insert(self.next_reliable_message_id, unacked_message);
        self.next_reliable_message_id += 1;
    }

    pub fn process_message_ack(&mut self, message_id: u64) {
        if self.unacked_messages.contains_key(&message_id) {
            let unacked_message = self.unacked_messages.remove(&message_id).unwrap();
            let UnackedMessage::Small { payload, .. } = unacked_message else {
                unreachable!("called ack on small message but found sliced");
            };
            self.memory_usage_bytes -= payload.len();
        }
    }

    pub fn process_slice_message_ack(&mut self, message_id: u64, slice_index: usize) {
        let Some(unacked_message) = self.unacked_messages.get_mut(&message_id) else {
            return;
        };

        let UnackedMessage::Sliced { payload, num_slices, num_acked_slices, acked, .. } = unacked_message else {
            unreachable!("called ack on sliced message but found small");
        };

        if acked[slice_index] {
            return;
        }

        acked[slice_index] = true;
        *num_acked_slices += 1;

        // TODO: actually divide the payload into slices, and then we can drop individual slices when acked
        // if slice_index == *num_slices - 1 {
        //     self.unacked_bytes -= payload.len() - (slice_index + 1) * SLICE_SIZE;
        // } else {
        //     self.unacked_bytes -= SLICE_SIZE;
        // }

        if *num_acked_slices == *num_slices {
            self.memory_usage_bytes -= payload.len();
            self.unacked_messages.remove(&message_id);
        }
    }
}

impl ReceiveChannelReliable {
    pub fn new(max_memory_usage_bytes: usize) -> Self {
        Self {
            slices: HashMap::new(),
            messages: BTreeMap::new(),
            next_message_id: 0,
            reliable_order: ReliableOrder::Ordered,
            memory_usage_bytes: 0,
            max_memory_usage_bytes,
            error: None,
        }
    }

    pub fn process_message(&mut self, message: Bytes, message_id: u64) {
        match &mut self.reliable_order {
            ReliableOrder::Ordered => {
                if !self.messages.contains_key(&message_id) {
                    self.memory_usage_bytes += message.len();
                    if self.max_memory_usage_bytes < self.memory_usage_bytes {
                        // FIXME: use correct error ::MaxMemoryReached
                        self.error = Some(ChannelError::SendQueueFull);
                    }

                    self.messages.insert(message_id, message);
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
                    received_messages.insert(message_id);

                    self.memory_usage_bytes += message.len();
                    if self.max_memory_usage_bytes < self.memory_usage_bytes {
                        // FIXME: use correct error ::MaxMemoryReached
                        self.error = Some(ChannelError::SendQueueFull);
                    }

                    self.messages.insert(message_id, message);
                }
            }
        }
    }

    pub fn process_slice(&mut self, slice: Slice) {
        if self.messages.contains_key(&slice.message_id) || slice.message_id < self.next_message_id {
            // Message already assembled
            return;
        }

        let slice_constructor = self.slices.entry(slice.message_id).or_insert_with(|| {
            self.memory_usage_bytes += slice.num_slices * SLICE_SIZE;
            if self.max_memory_usage_bytes < self.memory_usage_bytes {
                // FIXME: use correct error ::MaxMemoryReached
                self.error = Some(ChannelError::SendQueueFull);
            }

            SliceConstructor::new(slice.message_id, slice.num_slices)
        });

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                // Memory usage is readded with the exactly message size
                self.memory_usage_bytes -= slice.num_slices * SLICE_SIZE;
                self.process_message(message, slice.message_id);
                self.slices.remove(&slice.message_id);
            }
            Ok(None) => {}
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        match &mut self.reliable_order {
            ReliableOrder::Ordered => {
                let next_message_id = self.next_message_id;
                if !self.messages.contains_key(&next_message_id) {
                    return None;
                }

                self.next_message_id += 1;
                self.messages.remove(&next_message_id)
            }
            ReliableOrder::Unordered { received_messages, .. } => {
                let Some((message_id, message)) = self.messages.pop_first() else {
                    return None;
                };

                if self.next_message_id == message_id {
                    while received_messages.contains(&self.next_message_id) {
                        received_messages.remove(&message_id);
                        self.next_message_id += 1;
                    }
                }

                Some(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::packet;

    use super::*;

    #[test]
    fn small_packet() {
        let max_memory: usize = 10000;
        let mut sequence: u64 = 0;
        let mut current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut recv = ReceiveChannelReliable::new(max_memory);
        let mut send = SendChannelReliable::new(0, resend_time, max_memory);

        let message1 = vec![1, 2, 3];
        let message2 = vec![3, 4, 5];

        send.send_message(message1.clone().into());
        send.send_message(message2.clone().into());

        let packets = send.get_messages_to_send(&mut sequence, current_time);
        for packet in packets {
            let Packet::SmallReliable { packet_sequence: 0, channel_id: 0, messages } = packet else {
                unreachable!();
            };
            for (message, message_id) in messages {
                recv.process_message(message_id, message);
            }
        }

        let new_message1 = recv.receive_message().unwrap();
        let new_message2 = recv.receive_message().unwrap();

        assert_eq!(message1, new_message1);
        assert_eq!(message2, new_message2);

        // Should not resend anything
        let packets = send.get_messages_to_send(&mut sequence, current_time);
        assert!(packets.is_empty());

        current_time += resend_time;
        // Should resend now
        let packets = send.get_messages_to_send(&mut sequence, current_time);
        assert_eq!(packets.len(), 1);

        // Should not resend after ack
        current_time += resend_time;
        send.process_message_ack(0);
        send.process_message_ack(1);

        let packets = send.get_messages_to_send(&mut sequence, current_time);
        assert!(packets.is_empty());
    }

    #[test]
    fn slice_packet() {
        let max_memory: usize = 10000;
        let mut sequence: u64 = 0;
        let mut current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut recv = ReceiveChannelReliable::new(max_memory);
        let mut send = SendChannelReliable::new(0, resend_time, max_memory);

        let message = vec![5; SLICE_SIZE * 3];

        send.send_message(message.clone().into());

        let packets = send.get_messages_to_send(&mut sequence, current_time);
        for packet in packets {
            let Packet::MessageSlice { channel_id: 0, slice, .. } = packet else {
                unreachable!();
            };
            recv.process_slice(slice);
        }

        let new_message = recv.receive_message().unwrap();
        assert_eq!(message, new_message);

        // Should not resend anything
        let packets = send.get_messages_to_send(&mut sequence, current_time);
        assert!(packets.is_empty());

        current_time += resend_time;
        // Should resend now
        let packets = send.get_messages_to_send(&mut sequence, current_time);
        assert_eq!(packets.len(), 3);

        // Should not resend after ack
        current_time += resend_time;
        send.process_slice_message_ack(0, 0);
        send.process_slice_message_ack(0, 1);
        send.process_slice_message_ack(0, 2);

        let packets = send.get_messages_to_send(&mut sequence, current_time);
        assert!(packets.is_empty());
    }

    #[test]
    fn max_memory() {
        let mut sequence: u64 = 0;
        let current_time: Duration = Duration::ZERO;
        let resend_time = Duration::from_millis(100);
        let mut recv = ReceiveChannelReliable::new(99);
        let mut send = SendChannelReliable::new(0, resend_time, 101);

        let message = vec![5; 100];

        send.send_message(message.clone().into());

        let packets = send.get_messages_to_send(&mut sequence, current_time);
        for packet in packets {
            let Packet::SmallReliable { packet_sequence: 0, channel_id: 0, messages } = packet else {
                unreachable!();
            };
            for (message, message_id) in messages {
                recv.process_message(message_id, message);
            }
        }

        // TODO: match with exact error
        assert!(recv.error.is_some());

        assert!(send.error.is_none());
        send.send_message(message.clone().into());
        assert!(send.error.is_some());
    }
}
