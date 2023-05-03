use bytes::Bytes;
use std::ops::Range;

pub type Payload = Vec<u8>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    pub message_id: u64,
    pub slice_index: usize,
    pub num_slices: usize,
    pub payload: Bytes,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    // Small messages in a reliable channel are aggregated and sent in this packet
    SmallReliable {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<(u64, Bytes)>,
    },
    // Small messages in a unreliable channel are aggregated and sent in this packet
    SmallUnreliable {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<Bytes>,
    },
    // A big unreliable message is sliced in multiples slice packets
    UnreliableSlice {
        packet_sequence: u64,
        channel_id: u8,
        slice: Slice,
    },
    // A big reliable messages is sliced in multiples slice packets
    ReliableSlice {
        packet_sequence: u64,
        channel_id: u8,
        slice: Slice,
    },
    // Contains the packets that were acked
    // Acks are saved in multiples ranges, all values in the ranges are considered acked.
    Ack {
        packet_sequence: u64,
        ack_ranges: Vec<Range<u64>>,
    },
}

impl Packet {
    pub fn is_ack_eliciting(&self) -> bool {
        match self {
            Packet::SmallReliable { .. } | Packet::ReliableSlice { .. } => true,
            _ => false,
        }
    }

    pub fn to_bytes(&self, b: &mut octets::OctetsMut) -> Result<usize, octets::BufferTooShortError> {
        let before = b.cap();

        match self {
            Packet::SmallReliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                b.put_u8(0)?;
                b.put_varint(*packet_sequence)?;
                b.put_u8(*channel_id)?;
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
                b.put_varint(*packet_sequence)?;
                b.put_u8(*channel_id)?;
                b.put_u16(messages.len() as u16)?;
                for message in messages {
                    b.put_varint(message.len() as u64)?;
                    b.put_bytes(message)?;
                }
            }
            Packet::ReliableSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                b.put_u8(2)?;
                b.put_varint(*packet_sequence)?;
                b.put_u8(*channel_id)?;
                b.put_varint(slice.message_id)?;
                b.put_varint(slice.slice_index as u64)?;
                b.put_varint(slice.num_slices as u64)?;
                b.put_varint(slice.payload.len() as u64)?;
                b.put_bytes(&slice.payload)?;
            }
            Packet::UnreliableSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                b.put_u8(3)?;
                b.put_varint(*packet_sequence)?;
                b.put_u8(*channel_id)?;
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
                b.put_u8(4)?;
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
                //   |   | |  |  +-> 10: size of 20010..20020
                //   |   | |  +----> 15: gap between ranges 20010..20020 and 20035..20040
                //   |   | +--------> 1: remaing number of ranges
                //   |   +----------> 5: size of 20035..20040
                //   +----------> 20040:  end of 20035..20040
                //
                // We can always reconstruct the ranges using the start of the previous one and the gap.

                // Iterate in reverse order
                let mut it = ack_ranges.iter().rev();

                // Extract the last range (first in the iterator)
                let last = it.next().unwrap();
                let last_range_size = (last.end - 1) - last.start;

                b.put_varint(last.end - 1)?;
                b.put_varint(last_range_size)?;

                // Write the number of remaining ranges
                b.put_varint(it.len() as u64)?;

                let mut previous_range_start = last.start;
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
        }

        Ok(before - b.cap())
    }

    pub fn from_bytes(b: &mut octets::Octets) -> Result<Packet, octets::BufferTooShortError> {
        let packet_type = b.get_u8()?;
        match packet_type {
            0 => {
                // SmallReliable
                let packet_sequence = b.get_varint()?;
                let channel_id = b.get_u8()?;
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
                let packet_sequence = b.get_varint()?;
                let channel_id = b.get_u8()?;
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
                // ReliableSlice
                let packet_sequence = b.get_varint()?;
                let channel_id = b.get_u8()?;
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
                Ok(Packet::ReliableSlice {
                    packet_sequence,
                    channel_id,
                    slice,
                })
            }
            3 => {
                // UnreliableSlice
                let packet_sequence = b.get_varint()?;
                let channel_id = b.get_u8()?;
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
                Ok(Packet::UnreliableSlice {
                    packet_sequence,
                    channel_id,
                    slice,
                })
            }
            4 => {
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
            _ => Err(octets::BufferTooShortError), // TODO: correct error (invalid packet type)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::channels::SLICE_SIZE;

    use super::*;

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
    fn serialize_reliable_slice_packet() {
        let mut buffer = [0u8; 1300];

        let packet = Packet::ReliableSlice {
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
    fn serialize_unreliable_slice_packet() {
        let mut buffer = [0u8; 1300];

        let packet = Packet::UnreliableSlice {
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
}
