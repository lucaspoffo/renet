use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    ops::Range,
    time::Duration,
};

use bytes::Bytes;

use crate::{
    error::ChannelError,
};

pub const SLICE_SIZE: usize = 1200;

struct Ack {
    channel_id: u8,
    most_recent_packet: u64,
}

pub enum SmallMessage {
    Reliable { message_id: u64, payload: Bytes },
    Unreliable { payload: Bytes },
}

#[derive(Debug, Clone)]
pub struct Slice {
    pub message_id: u64,
    pub slice_index: usize,
    pub num_slices: usize,
    pub payload: Bytes,
}

#[derive(Debug, Clone)]
enum PacketSent {
    ReliableMessages { channel_id: u8, message_ids: Vec<u64> },
    ReliableSliceMessage { channel_id: u8, message_id: u64, slice_index: u64 },
    // When an ack packet is acknowledged,
    // We remove all Ack ranges below the largest_acked sent by it
    Ack { largest_acked_packet: u64 },
}

pub enum Packet {
    Normal {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<SmallMessage>,
    },
    SmallReliable {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<(u64, Bytes)>,
    },
    SmallUnreliable {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<Bytes>
    },
    SmallUnreliableSequenced {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<(u64, Bytes)>
    },
    MessageSlice {
        packet_sequence: u64,
        channel_id: u8,
        slice: Slice,
    },
    Ack {
        packet_sequence: u64,
        ack_ranges: Vec<Range<u64>>,
    },
    Disconnect,
}

struct PendingMessage {
    payload: Bytes,
    last_sent: Option<Duration>,
}

struct PendingSlicedMessage {
    payload: Bytes,
    num_slices: usize,
    current_slice_id: usize,
    num_acked_slices: usize,
    acked: Vec<bool>,
    last_sent: Vec<Option<Duration>>,
}

struct Connection {
    // packet_sequence: u64,
    // packets_sent: SequenceBuffer<PacketSent>,
    sent_packets: BTreeMap<u64, PacketSent>,
    pending_acks: Vec<Range<u64>>,
    // receive_channels: HashMap<u8, ReceiveChannel>,
    // send_channels: HashMap<u8, ReceiveChannel>,
}

impl Connection {
    fn new() -> Self {
        Self {
            // send_channels: HashMap::new(),
            sent_packets: BTreeMap::new(),
            pending_acks: Vec::new(),
            // receive_channels: HashMap::new(),
        }
    }

    fn process_packet(&mut self, packet: Packet, current_time: Duration) {
        match packet {
            Packet::Normal {
                packet_sequence,
                channel_id,
                messages,
            } => {
                self.add_pending_ack(packet_sequence);
                // let Some(receive_channel) = self.receive_channels.get_mut(&channel_id) else {
                    // Receive message without channel, should error
                    // return;
                // };

                // for message in messages {
                    // receive_channel.process_message(message, current_time);
                // }
            }
            Packet::MessageSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                self.add_pending_ack(packet_sequence);
                // let Some(receive_channel) = self.receive_channels.get_mut(&channel_id) else {
                    // Receive message without channel, should error
                    // return;
                // };

                // match reliable {
                    // true => receive_channel.process_reliable_slice(slice, current_time),
                    // false => receive_channel.process_unreliable_slice(slice, current_time),
                // }
            }
            Packet::Ack {
                packet_sequence,
                ack_ranges,
            } => {
                self.add_pending_ack(packet_sequence);

                for range in ack_ranges {
                    for packet_sequence in range {
                        if let Some(sent_packet) = self.sent_packets.remove(&packet_sequence) {
                            match sent_packet {
                                PacketSent::ReliableMessages { channel_id, message_ids } => {
                                    // let send_channel = self.send_channels.get_mut(&channel_id).unwrap();
                                },
                                PacketSent::ReliableSliceMessage { channel_id, message_id, slice_index } => todo!(),
                                PacketSent::Ack { largest_acked_packet } => {
                                    self.acked_largest(largest_acked_packet);
                                },
                            }
                        }
                    }
                }
            }
            Packet::Disconnect => todo!(),
        }
        // Ack logic
    }

    fn add_pending_ack(&mut self, sequence: u64) {
        if self.pending_acks.is_empty() {
            self.pending_acks.push(sequence..sequence + 1);
            return;
        }

        for index in 0..self.pending_acks.len() {
            let range = &mut self.pending_acks[index];
            if range.contains(&sequence) {
                // Sequence already contained in this range
                return;
            }

            if range.start == sequence + 1 {
                // New sequence is just before this range
                range.start = sequence;
                return;
            } else if range.end == sequence {
                // New sequence is just after this range
                range.end = sequence + 1;

                // Check if we can merge with the range just after it
                let next_index = index + 1;
                if next_index < self.pending_acks.len() && self.pending_acks[index].end == self.pending_acks[next_index].start {
                    self.pending_acks[index].end = self.pending_acks[next_index].end;
                    self.pending_acks.remove(next_index);
                }

                return;
            } else if self.pending_acks[index].start > sequence + 1 {
                // New sequence is before this range and not extensible to it
                // Add new range to the left
                self.pending_acks.insert(index, sequence..sequence + 1);
                return;
            }
        }

        // New sequence was not before or adjacent to any range
        // Add new range with only this sequence at the end
        self.pending_acks.push(sequence..sequence + 1);
    }

    fn acked_largest(&mut self, largest_ack: u64) {
        while self.pending_acks.len() != 0 {
            let range = &mut self.pending_acks[0];

            // Largest ack is below the range, stop checking
            if largest_ack < range.start {
                return;
            }

            // Largest ack is above the range, remove it
            if range.end <= largest_ack {
                self.pending_acks.remove(0);
                continue;
            }

            // Largest ack is contained in the range
            // Update start
            range.start = largest_ack + 1;
            if range.is_empty() {
                self.pending_acks.remove(0);
            }

            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Connection;

    #[test]
    fn pending_acks() {
        let mut connection = Connection::new();
        connection.add_pending_ack(3);
        assert_eq!(connection.pending_acks, vec![3..4]);

        connection.add_pending_ack(4);
        assert_eq!(connection.pending_acks, vec![3..5]);

        connection.add_pending_ack(2);
        assert_eq!(connection.pending_acks, vec![2..5]);

        connection.add_pending_ack(0);
        assert_eq!(connection.pending_acks, vec![0..1, 2..5]);

        connection.add_pending_ack(7);
        assert_eq!(connection.pending_acks, vec![0..1, 2..5, 7..8]);

        connection.add_pending_ack(1);
        assert_eq!(connection.pending_acks, vec![0..5, 7..8]);

        connection.add_pending_ack(5);
        assert_eq!(connection.pending_acks, vec![0..6, 7..8]);

        connection.add_pending_ack(6);
        assert_eq!(connection.pending_acks, vec![0..8]);
    }

    #[test]
    fn ack_pending_acks() {
        let mut connection = Connection::new();
        for i in 0..10 {
            connection.add_pending_ack(i);
        }

        assert_eq!(connection.pending_acks, vec![0..10]);

        connection.acked_largest(0);
        assert_eq!(connection.pending_acks, vec![1..10]);

        connection.acked_largest(3);
        assert_eq!(connection.pending_acks, vec![4..10]);

        connection.add_pending_ack(0);
        assert_eq!(connection.pending_acks, vec![0..1, 4..10]);
        connection.acked_largest(5);
        assert_eq!(connection.pending_acks, vec![6..10]);

        connection.add_pending_ack(0);
        assert_eq!(connection.pending_acks, vec![0..1, 6..10]);
        connection.acked_largest(15);
        assert_eq!(connection.pending_acks, vec![]);
    }
}
