use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    time::Duration,
};

use bytes::Bytes;

use crate::channels::{
    reliable::{ReceiveChannelReliable, SendChannelReliable},
    unreliable::{ReceiveChannelUnreliable, SendChannelUnreliable},
    unreliable_sequenced::{ReceiveChannelUnreliableSequenced, SendChannelUnreliableSequenced},
};

#[derive(Debug, Clone)]
pub struct Slice {
    pub message_id: u64,
    pub slice_index: usize,
    pub num_slices: usize,
    pub payload: Bytes,
}

#[derive(Debug, Clone)]
enum PacketSent {
    ReliableMessages {
        channel_id: u8,
        message_ids: Vec<u64>,
    },
    ReliableSliceMessage {
        channel_id: u8,
        message_id: u64,
        slice_index: usize,
    },
    // When an ack packet is acknowledged,
    // We remove all Ack ranges below the largest_acked sent by it
    Ack {
        largest_acked_packet: u64,
    },
}

pub enum Packet {
    SmallReliable {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<(u64, Bytes)>,
    },
    SmallUnreliable {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<Bytes>,
    },
    SmallUnreliableSequenced {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<(u64, Bytes)>,
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

struct Connection {
    // packet_sequence: u64,
    // packets_sent: SequenceBuffer<PacketSent>,
    sent_packets: BTreeMap<u64, PacketSent>,
    // Pending acks are saved as ranges,
    // new acks create a new range or are appended to already created
    pending_acks: Vec<Range<u64>>,
    send_unreliable_channels: HashMap<u8, SendChannelUnreliable>,
    receive_unreliable_channels: HashMap<u8, ReceiveChannelUnreliable>,
    send_unreliable_sequenced_channels: HashMap<u8, SendChannelUnreliableSequenced>,
    receive_unreliable_sequenced_channels: HashMap<u8, ReceiveChannelUnreliableSequenced>,
    send_reliable_channels: HashMap<u8, SendChannelReliable>,
    receive_reliable_channels: HashMap<u8, ReceiveChannelReliable>,
}

impl Connection {
    fn new() -> Self {
        Self {
            sent_packets: BTreeMap::new(),
            pending_acks: Vec::new(),
            send_unreliable_channels: todo!(),
            receive_unreliable_channels: todo!(),
            send_unreliable_sequenced_channels: todo!(),
            receive_unreliable_sequenced_channels: todo!(),
            send_reliable_channels: todo!(),
            receive_reliable_channels: todo!(),
        }
    }

    fn process_packet(&mut self, packet: Packet, current_time: Duration) {
        match packet {
            Packet::SmallReliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_reliable_channels.get_mut(&channel_id) else {
                    // TODO: self.error = channel not found;
                    return;
                };

                for (message_id, message) in messages {
                    channel.process_message(message, message_id);
                }
            }
            Packet::SmallUnreliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_unreliable_channels.get_mut(&channel_id) else {
                    // TODO: self.error = channel not found;
                    return;
                };

                for message in messages {
                    channel.process_message(message);
                }
            }
            Packet::SmallUnreliableSequenced {
                packet_sequence,
                channel_id,
                messages,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_unreliable_sequenced_channels.get_mut(&channel_id) else {
                    // TODO: self.error = channel not found;
                    return;
                };

                for (message_id, message) in messages {
                    channel.process_message(message, message_id);
                }
            }
            Packet::MessageSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                self.add_pending_ack(packet_sequence);
                if let Some(channel) = self.receive_unreliable_sequenced_channels.get_mut(&channel_id) {
                    channel.process_slice(slice, current_time);
                } else if let Some(channel) = self.receive_unreliable_channels.get_mut(&channel_id) {
                    channel.process_slice(slice, current_time);
                } else if let Some(channel) = self.receive_reliable_channels.get_mut(&channel_id) {
                    channel.process_slice(slice);
                } else {
                    // TODO: self.error = channel not found;
                }
            }
            Packet::Ack {
                packet_sequence,
                ack_ranges,
            } => {
                self.add_pending_ack(packet_sequence);

                // Create list with just new acks
                // This prevents DoS from huge ack ranges
                let mut new_acks: Vec<u64> = Vec::new();
                for range in ack_ranges {
                    for (&sequence, _) in self.sent_packets.range(range) {
                        new_acks.push(sequence)
                    }
                }

                for packet_sequence in new_acks {
                    let sent_packet = self.sent_packets.remove(&packet_sequence).unwrap();

                    match sent_packet {
                        PacketSent::ReliableMessages { channel_id, message_ids } => {
                            let reliable_channel = self.send_reliable_channels.get_mut(&channel_id).unwrap();
                            for message_id in message_ids {
                                reliable_channel.process_message_ack(message_id);
                            }
                        }
                        PacketSent::ReliableSliceMessage {
                            channel_id,
                            message_id,
                            slice_index,
                        } => {
                            let reliable_channel = self.send_reliable_channels.get_mut(&channel_id).unwrap();
                            reliable_channel.process_slice_message_ack(message_id, slice_index);
                        }
                        PacketSent::Ack { largest_acked_packet } => {
                            self.acked_largest(largest_acked_packet);
                        }
                    }
                }
            }
            Packet::Disconnect => todo!(),
        }
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
