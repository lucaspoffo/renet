use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    time::Duration,
};

use bytes::Bytes;
use octets::OctetsMut;

use crate::channels::{
    reliable::{ReceiveChannelReliable, SendChannelReliable},
    unreliable::{ReceiveChannelUnreliable, SendChannelUnreliable},
};

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, PartialEq, Eq)]
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

impl Packet {
    pub fn to_bytes(&self, b: &mut octets::OctetsMut) -> Result<usize, octets::BufferTooShortError> {
        let before = b.cap();

        match self {
            Packet::SmallReliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                b.put_u8(0)?;
                b.put_u8(*channel_id)?;
                b.put_varint(*packet_sequence)?;
                b.put_u16(messages.len() as u16)?;
                for (message_id, message) in messages {
                    b.put_varint(*message_id)?;
                    b.put_varint(message.len() as u64)?;
                    b.put_bytes(message)?;
                }
            }
            Packet::SmallUnreliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                b.put_u8(1)?;
                b.put_u8(*channel_id)?;
                b.put_varint(*packet_sequence)?;
                b.put_u16(messages.len() as u16)?;
                for message in messages {
                    b.put_varint(message.len() as u64)?;
                    b.put_bytes(message)?;
                }
            }
            Packet::MessageSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                b.put_u8(2)?;
                b.put_u8(*channel_id)?;
                b.put_varint(*packet_sequence)?;
                b.put_varint(slice.message_id)?;
                b.put_varint(slice.slice_index as u64)?;
                b.put_varint(slice.num_slices as u64)?;
                b.put_varint(slice.payload.len() as u64)?;
                b.put_bytes(&slice.payload)?;
            }
            Packet::Ack {
                packet_sequence,
                ack_ranges,
            } => {
                b.put_u8(3)?;
                b.put_varint(*packet_sequence)?;

                // Consider this ranges:
                // [20010..20020   ,  20035..20040]
                //  <----10----><-15-><----5------>
                //
                // We can represented more compactly each range if we serialize it based
                // on the start of the previous one, since the difference is usually small
                // The ranges would become before serializing:
                // 20040 5 1 15 10
                //   |   | |  |  |
                //   |   | |  |  +-> 10: size of 20017..20020
                //   |   | |  +----> 15: gap between ranges 20010..20020 and 20035..20040
                //   |   | +--------> 1: remaing number of ranges
                //   |   +----------> 5: size of 20035..20040
                //   +----------> 20040:  end of 20035..20040
                //
                // We can always reconstruct the ranges using the start of the previous one and the gap.

                // Iterate in reverse order
                let mut it = ack_ranges.iter().rev();

                // Extract the first range
                let first = it.next().unwrap();
                let first_range_size = (first.end - 1) - first.start;

                b.put_varint(first.end - 1)?;
                b.put_varint(first_range_size)?;

                // Write the number of remaining ranges
                b.put_varint(it.len() as u64)?;

                let mut previous_range_start = first.start;
                // For each subsequent range:
                for range in it {
                    // Calculate the gap between the start of the previous range and the end of the current range
                    let gap = previous_range_start - range.end - 1;
                    let range_size = (range.end - 1) - range.start;

                    b.put_varint(gap)?;
                    b.put_varint(range_size)?;

                    previous_range_start = range.start;
                }
            }
            Packet::Disconnect => {
                b.put_u8(4)?;
            }
        }

        Ok(before - b.cap())
    }

    pub fn from_bytes(b: &mut octets::Octets) -> Result<Packet, octets::BufferTooShortError> {
        let packet_type = b.get_u8()?;
        match packet_type {
            0 => {
                // SmallReliable
                let channel_id = b.get_u8()?;
                let packet_sequence = b.get_varint()?;
                let messages_len = b.get_u16()?;
                let mut messages: Vec<(u64, Bytes)> = Vec::with_capacity(64);
                for _ in 0..messages_len {
                    let message_id = b.get_varint()?;
                    let payload = b.get_bytes_with_varint_length()?;

                    messages.push((message_id, payload.to_vec().into()));
                }

                Ok(Packet::SmallReliable {
                    packet_sequence,
                    channel_id,
                    messages,
                })
            }
            1 => {
                // SmallUnreliable
                let channel_id = b.get_u8()?;
                let packet_sequence = b.get_varint()?;
                let messages_len = b.get_u16()?;
                let mut messages: Vec<Bytes> = Vec::with_capacity(64);
                for _ in 0..messages_len {
                    let payload = b.get_bytes_with_varint_length()?;
                    messages.push(payload.to_vec().into());
                }

                Ok(Packet::SmallUnreliable {
                    packet_sequence,
                    channel_id,
                    messages,
                })
            }
            2 => {
                // MessageSlice
                let channel_id = b.get_u8()?;
                let packet_sequence = b.get_varint()?;
                let message_id = b.get_varint()?;
                let slice_index = b.get_varint()? as usize;
                let num_slices = b.get_varint()? as usize;
                let payload = b.get_bytes_with_varint_length()?;

                let slice = Slice {
                    message_id,
                    slice_index,
                    num_slices,
                    payload: payload.to_vec().into(),
                };
                Ok(Packet::MessageSlice {
                    packet_sequence,
                    channel_id,
                    slice,
                })
            }
            3 => {
                // Ack
                let packet_sequence = b.get_varint()?;

                let first_range_end = b.get_varint()?;
                let first_range_size = b.get_varint()?;
                let num_remaining_ranges = b.get_varint()?;

                if first_range_end < first_range_size {
                    // TODO: Invalid ack packet
                    return Err(octets::BufferTooShortError);
                }

                let mut ranges: Vec<Range<u64>> = Vec::with_capacity(32);

                let first_range_start = first_range_end - first_range_size;
                ranges.push(first_range_start..first_range_end + 1);

                let mut previous_range_start = first_range_start;
                for _ in 0..num_remaining_ranges {
                    // Get the gap between the previous range and the current one
                    let gap = b.get_varint()?;

                    if previous_range_start < 2 + gap {
                        // TODO: Invalid ack packet
                        return Err(octets::BufferTooShortError);
                    }

                    // Get the end of the current range using the start of the previous one and the gap
                    let range_end = (previous_range_start - gap) - 2;
                    let range_size = b.get_varint()?;

                    if range_end < range_size {
                        // TODO: Invalid ack packet
                        return Err(octets::BufferTooShortError);
                    }

                    let range_start = range_end - range_size;
                    ranges.push(range_start..range_end + 1);

                    previous_range_start = range_start;
                }

                ranges.reverse();

                Ok(Packet::Ack {
                    packet_sequence,
                    ack_ranges: ranges,
                })
            }
            4 => {
                // Disconnect
                Ok(Packet::Disconnect)
            }
            _ => Err(octets::BufferTooShortError), // TODO: correct error (invalid packet type)
        }
    }
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
    send_reliable_channels: HashMap<u8, SendChannelReliable>,
    receive_reliable_channels: HashMap<u8, ReceiveChannelReliable>,
}

impl Connection {
    fn new() -> Self {
        Self {
            sent_packets: BTreeMap::new(),
            pending_acks: Vec::new(),
            send_unreliable_channels: HashMap::new(),
            receive_unreliable_channels: HashMap::new(),
            send_reliable_channels: HashMap::new(),
            receive_reliable_channels: HashMap::new(),
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
            Packet::MessageSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                self.add_pending_ack(packet_sequence);
                if let Some(channel) = self.receive_unreliable_channels.get_mut(&channel_id) {
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
    use crate::channels::{SliceConstructor, SLICE_SIZE};

    use super::*;

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

    #[test]
    fn serialize_small_reliable_packet() {
        let mut buffer = [0u8; 1300];
        let packet = Packet::SmallReliable {
            packet_sequence: 0,
            channel_id: 0,
            messages: vec![(0, vec![0, 0, 0].into()), (1, vec![1, 1, 1].into()), (2, vec![2, 2, 2].into())],
        };

        let mut b = octets::OctetsMut::with_slice(&mut buffer);
        packet.to_bytes(&mut b).unwrap();

        let mut b = octets::Octets::with_slice(&buffer);
        let recv_packet = Packet::from_bytes(&mut b).unwrap();
        assert_eq!(packet, recv_packet);
    }

    #[test]
    fn serialize_small_unreliable_packet() {
        let mut buffer = [0u8; 1300];
        let packet = Packet::SmallUnreliable {
            packet_sequence: 0,
            channel_id: 0,
            messages: vec![vec![0, 0, 0].into(), vec![1, 1, 1].into(), vec![2, 2, 2].into()],
        };

        let mut b = octets::OctetsMut::with_slice(&mut buffer);
        packet.to_bytes(&mut b).unwrap();

        let mut b = octets::Octets::with_slice(&buffer);
        let recv_packet = Packet::from_bytes(&mut b).unwrap();
        assert_eq!(packet, recv_packet);
    }

    #[test]
    fn serialize_slice_packet() {
        let mut buffer = [0u8; 1300];

        let packet = Packet::MessageSlice {
            packet_sequence: 0,
            channel_id: 0,
            slice: Slice {
                message_id: 0,
                slice_index: 0,
                num_slices: 0,
                payload: vec![5; SLICE_SIZE].into(),
            },
        };

        let mut b = octets::OctetsMut::with_slice(&mut buffer);
        packet.to_bytes(&mut b).unwrap();

        let mut b = octets::Octets::with_slice(&buffer);
        let recv_packet = Packet::from_bytes(&mut b).unwrap();
        assert_eq!(packet, recv_packet);
    }

    #[test]
    fn serialize_ack_packet() {
        let mut buffer = [0u8; 1300];

        let packet = Packet::Ack {
            packet_sequence: 0,
            ack_ranges: vec![3..7, 10..20, 30..100],
        };

        let mut b = octets::OctetsMut::with_slice(&mut buffer);
        packet.to_bytes(&mut b).unwrap();

        let mut b = octets::Octets::with_slice(&buffer);
        let recv_packet = Packet::from_bytes(&mut b).unwrap();
        assert_eq!(packet, recv_packet);
    }

    #[test]
    fn serialize_disconnect_packet() {
        let mut buffer = [0u8; 1300];
        let packet = Packet::Disconnect;

        let mut b = octets::OctetsMut::with_slice(&mut buffer);
        packet.to_bytes(&mut b).unwrap();

        let mut b = octets::Octets::with_slice(&buffer);
        let recv_packet = Packet::from_bytes(&mut b).unwrap();
        assert_eq!(packet, recv_packet);
    }
}
