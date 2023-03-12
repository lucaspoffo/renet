use core::slice;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    ops::Range,
    time::Duration,
};

use bytes::Bytes;

use crate::{
    error::ChannelError,
    packet::AckData,
    sequence_buffer::{sequence_greater_than, sequence_less_than, SequenceBuffer},
};

const SLICE_SIZE: usize = 400;

struct Ack {
    channel_id: u8,
    most_recent_packet: u16,
}

enum Message {
    Reliable { message_id: u16, payload: Bytes },
    Unreliable { payload: Bytes },
}

#[derive(Debug, Clone)]
struct Slice {
    message_id: u16,
    slice_index: u16,
    num_slices: u16,
    payload: Bytes,
}

#[derive(Debug, Clone)]
struct SliceConstructor {
    message_id: u16,
    num_slices: usize,
    num_received_slices: usize,
    received: Vec<bool>,
    sliced_data: Vec<u8>,
}

struct ReceiveChannel {
    unreliable_slices: HashMap<u16, SliceConstructor>,
    unreliable_slices_last_received: HashMap<u16, Duration>,
    unreliable_messages: VecDeque<Vec<u8>>,
    reliable_slices: HashMap<u16, SliceConstructor>,
    reliable_messages: SequenceBuffer<Vec<u8>>,
    next_reliable_message_id: u16,
    oldest_unacked_packet: u16,
    error: Option<ChannelError>,
}

#[derive(Debug, Clone)]
struct SentPacket {
    reliable_messages_id: Vec<u16>,
    // TODO: use SmallVec<3>, only 3 slices will fit in a packet
    slice_messages_id: Vec<u16>,
}

#[derive(Debug, Clone)]
enum PacketSent {
    ReliableMessages { channel_id: u8, message_ids: Vec<u16> },
    ReliableSliceMessage { channel_id: u8, message_id: u16, slice_index: u16 },
    // When an ack packet is acknowledged,
    // We remove all Ack ranges below the largest_acked sent by it
    Ack { largest_acked: u64 },
}

enum Packet {
    Normal {
        packet_sequence: u64,
        channel_id: u8,
        messages: Vec<Message>,
    },
    MessageSlice {
        packet_sequence: u64,
        channel_id: u8,
        reliable: bool,
        slice: Slice,
    },
    Ack {
        packet_sequence: u64,
        ack_ranges: Vec<Range<u64>>,
    },
    Disconnect,
}

#[derive(Debug, Clone)]
enum PendingReliable {
    Normal {
        payload: Bytes,
        last_sent: Option<Duration>,
    },
    Sliced {
        payload: Bytes,
        num_slices: usize,
        current_slice_id: usize,
        num_acked_slices: usize,
        acked: Vec<bool>,
        last_sent: Vec<Option<Duration>>,
    },
}

struct SendChannel {
    unreliable_messages: VecDeque<Vec<u8>>,
    reliable_messages: SequenceBuffer<PendingReliable>,
    reliable_message_id: u16,
    unreliable_message_id: u16,
    oldest_unacked_message_id: u16,
    resend_time: Duration,
    error: Option<ChannelError>,
}

struct Connection {
    // packet_sequence: u64,
    // packets_sent: SequenceBuffer<PacketSent>,
    sent_packets: BTreeMap<u64, PacketSent>,
    pending_acks: Vec<Range<u64>>,
    receive_channels: HashMap<u8, ReceiveChannel>,
    send_channels: HashMap<u8, ReceiveChannel>,
}

impl Connection {
    fn new() -> Self {
        Self {
            send_channels: HashMap::new(),
            sent_packets: BTreeMap::new(),
            pending_acks: Vec::new(),
            receive_channels: HashMap::new(),
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
                let Some(receive_channel) = self.receive_channels.get_mut(&channel_id) else {
                    // Receive message without channel, should error
                    return;
                };

                for message in messages {
                    receive_channel.process_message(message, current_time);
                }
            }
            Packet::MessageSlice {
                packet_sequence,
                channel_id,
                reliable,
                slice,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(receive_channel) = self.receive_channels.get_mut(&channel_id) else {
                    // Receive message without channel, should error
                    return;
                };

                match reliable {
                    true => receive_channel.process_reliable_slice(slice, current_time),
                    false => receive_channel.process_unreliable_slice(slice, current_time),
                }
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
                                    let send_channel = self.send_channels.get_mut(&channel_id).unwrap();

                                },
                                PacketSent::ReliableSliceMessage { channel_id, message_id, slice_index } => todo!(),
                                PacketSent::Ack { largest_acked } => {
                                    self.acked_largest(largest_acked);
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

impl SendChannel {
    fn process_reliable_message_ack(&mut self, message_id: u16) {
        if self.reliable_messages.exists(message_id) {
            let pending_message = self.reliable_messages.remove(message_id);
            if let Some(PendingReliable::Sliced { .. }) = pending_message {
                // TODO: better error
                self.error = Some(ChannelError::InvalidSliceMessage);
            }
        }
    }

    fn process_reliable_slice_message_ack(&mut self, message_id: u16, slice_index: u32) {
        if let Some(pending_message) = self.reliable_messages.get_mut(message_id) {
            match pending_message {
                PendingReliable::Normal { payload, last_sent } => {
                    // TODO: better error
                    self.error = Some(ChannelError::InvalidSliceMessage);
                },
                PendingReliable::Sliced { payload, num_slices, current_slice_id, num_acked_slices, acked, last_sent } => todo!(),
            }
        }
        if self.reliable_messages.exists(message_id) {
            let pending_message = self.reliable_messages.remove(message_id);
            if let Some(PendingReliable::Sliced { .. }) = pending_message {
                // TODO: better error
                self.error = Some(ChannelError::InvalidSliceMessage);
            }
        }
    }
}

impl ReceiveChannel {
    fn new() -> Self {
        Self {
            unreliable_slices: HashMap::new(),
            unreliable_slices_last_received: HashMap::new(),
            unreliable_messages: VecDeque::new(),
            reliable_slices: HashMap::new(),
            reliable_messages: SequenceBuffer::with_capacity(256),
            next_reliable_message_id: 0,
            oldest_unacked_packet: 0,
            error: None,
        }
    }

    fn process_message(&mut self, message: Message, current_time: Duration) {
        match message {
            Message::Reliable { message_id, payload } => {
                if sequence_less_than(message_id, self.next_reliable_message_id) {
                    // Discard old message
                    return;
                }

                let max_message_id = self.next_reliable_message_id.wrapping_add(self.reliable_messages.size() as u16 - 1);
                if sequence_greater_than(message_id, max_message_id) {
                    // Out of space to to add messages
                    // TODO: rename to ReliableMessageOutQueueSpace
                    self.error = Some(ChannelError::ReliableChannelOutOfSync);
                    return;
                }

                if !self.reliable_messages.exists(message_id) {
                    self.reliable_messages.insert(message_id, payload.into());
                }
            }
            Message::Unreliable { payload } => {
                self.unreliable_messages.push_back(payload.into());
            }
        }
    }

    fn process_reliable_slice(&mut self, slice: Slice, current_time: Duration) {
        if self.reliable_messages.exists(slice.message_id) {
            // Message already assembled
            return;
        }

        // TODO: remove duplication from process_message
        // TODO: add struct Sequence(u16) add Sequence::is_less_than
        if sequence_less_than(slice.message_id, self.next_reliable_message_id) {
            // Discard old message
            return;
        }

        let slice_constructor = self
            .reliable_slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                // TODO: remove duplication from process_message
                let max_message_id = self.next_reliable_message_id.wrapping_add(self.reliable_messages.size() as u16 - 1);
                if sequence_greater_than(slice.message_id, max_message_id) {
                    // Out of space to to add messages
                    // TODO: rename to ReliableMessageOutQueueSpace
                    self.error = Some(ChannelError::ReliableChannelOutOfSync);
                    return;
                }
                self.reliable_slices.remove(&slice.message_id);
                self.reliable_messages.insert(slice.message_id, message);
            }
            Ok(None) => {}
        }
    }

    fn process_unreliable_slice(&mut self, slice: Slice, current_time: Duration) {
        let slice_constructor = self
            .unreliable_slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.unreliable_slices.remove(&slice.message_id);
                self.unreliable_slices_last_received.remove(&slice.message_id);
                self.unreliable_messages.push_back(message);
            }
            Ok(None) => {
                self.unreliable_slices_last_received.insert(slice.message_id, current_time);
            }
        }
    }
}

impl SliceConstructor {
    fn new(message_id: u16, num_slices: u16) -> Self {
        SliceConstructor {
            message_id,
            num_slices: num_slices as usize,
            num_received_slices: 0,
            received: vec![false; num_slices as usize],
            sliced_data: vec![0; num_slices as usize * SLICE_SIZE],
        }
    }

    fn process_slice(&mut self, slice_index: u16, bytes: &[u8]) -> Result<Option<Vec<u8>>, ChannelError> {
        let slice_index = slice_index as usize;

        let is_last_slice = slice_index == self.num_slices - 1;
        if is_last_slice {
            if bytes.len() > SLICE_SIZE {
                log::error!(
                    "Invalid last slice_size for SliceMessage, got {}, expected less than {}.",
                    bytes.len(),
                    SLICE_SIZE,
                );
                return Err(ChannelError::InvalidSliceMessage);
            }
        } else if bytes.len() != SLICE_SIZE {
            log::error!("Invalid slice_size for SliceMessage, got {}, expected {}.", bytes.len(), SLICE_SIZE);
            return Err(ChannelError::InvalidSliceMessage);
        }

        if !self.received[slice_index] {
            self.received[slice_index] = true;
            self.num_received_slices += 1;

            if is_last_slice {
                let len = (self.num_slices - 1) * SLICE_SIZE + bytes.len();
                self.sliced_data.resize(len, 0);
            }

            let start = slice_index * SLICE_SIZE;
            let end = if slice_index == self.num_slices - 1 {
                (self.num_slices - 1) * SLICE_SIZE + bytes.len()
            } else {
                (slice_index + 1) * SLICE_SIZE
            };

            self.sliced_data[start..end].copy_from_slice(&bytes);
            log::trace!(
                "Received slice {} from message {}. ({}/{})",
                slice_index,
                self.message_id,
                self.num_received_slices,
                self.num_slices
            );
        }

        if self.num_received_slices == self.num_slices {
            log::trace!("Received all slices for message {}.", self.message_id);
            let payload = std::mem::take(&mut self.sliced_data);
            return Ok(Some(payload));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use crate::packet::AckData;

    use super::{Connection, ReceiveChannel};

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
