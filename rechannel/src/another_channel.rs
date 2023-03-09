use core::slice;
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    error::ChannelError,
    sequence_buffer::{sequence_greater_than, sequence_less_than, SequenceBuffer}, packet::AckData,
};

const SLICE_SIZE: usize = 400;

struct Ack {
    channel_id: u8,
    most_recent_packet: u16,
}

struct Message {
    message_type: MessageType,
    payload: Bytes,
}

#[derive(Debug, Clone)]
enum MessageType {
    Unreliable,
    Reliable {
        message_id: u16,
    },
    ReliableSliced {
        message_id: u16,
        slice_index: u16,
        num_slices: u16,
    },
    UnreliableSliced {
        message_id: u16,
        slice_index: u16,
        num_slices: u16,
    },
}

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
    received_packets: SequenceBuffer<()>,
    oldest_unacked_packet: u16,
    error: Option<ChannelError>,
}

#[derive(Debug, Clone)]
struct SentPacket {
    reliable_messages_id: Vec<u16>,
    // TODO: use SmallVec<3>, only 3 slices will fit in a packet
    slice_messages_id: Vec<u16>,
}

struct SendChannel {
    packets_sent: SequenceBuffer<SentPacket>,
}

impl SendChannel {
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
            received_packets: SequenceBuffer::with_capacity(256),
            error: None     
        }
    }

    fn generate_acks(&mut self) -> Vec<AckData> {
        let mut acks = Vec::new();
        let ack_group_count = 252usize / 32;
        let mut sequence = self.received_packets.sequence().wrapping_sub(1);
        for _ in 0..ack_group_count {
            if sequence < self.oldest_unacked_packet {
                break;
            }
            let ack = self.received_packets.ack_data(sequence);
            acks.push(ack);
            sequence = sequence.wrapping_sub(32);
        }

        acks
    }

    fn process_message(&mut self, message: Message, current_time: Duration) {
        if let MessageType::ReliableSliced { message_id, .. } | MessageType::Reliable { message_id, .. } = message.message_type {
            // TODO: add struct Sequence(u16) add Sequence::is_less_than
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
        }

        match message.message_type {
            MessageType::Unreliable => {
                self.unreliable_messages.push_back(message.payload.into());
            }
            MessageType::Reliable { message_id } => {
                if !self.reliable_messages.exists(message_id) {
                    self.reliable_messages.insert(message_id, message.payload.into());
                }
            }
            MessageType::ReliableSliced {
                message_id,
                slice_index,
                num_slices,
            } => {
                if !self.reliable_messages.exists(message_id) {
                    // Message already assembled
                    return;
                }

                let slice_constructor = self
                    .reliable_slices
                    .entry(message_id)
                    .or_insert_with(|| SliceConstructor::new(message_id, num_slices));

                match slice_constructor.process_slice(slice_index, &message.payload) {
                    Err(e) => self.error = Some(e),
                    Ok(Some(message)) => {
                        self.reliable_slices.remove(&message_id);
                        self.reliable_messages.insert(message_id, message);
                    }
                    Ok(None) => {}
                }
            }
            MessageType::UnreliableSliced {
                message_id,
                slice_index,
                num_slices,
            } => {
                let slice_constructor = self
                    .unreliable_slices
                    .entry(message_id)
                    .or_insert_with(|| SliceConstructor::new(message_id, num_slices));

                match slice_constructor.process_slice(slice_index, &message.payload) {
                    Err(e) => self.error = Some(e),
                    Ok(Some(message)) => {
                        self.unreliable_slices.remove(&message_id);
                        self.unreliable_slices_last_received.remove(&message_id);
                        self.unreliable_messages.push_back(message);
                    }
                    Ok(None) => {
                        self.unreliable_slices_last_received.insert(message_id, current_time);
                    }
                }
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

    use super::ReceiveChannel;

    #[test]
    fn generate_acks() {
        let mut receive_channel = ReceiveChannel::new();
        for i in 0..32 {
            receive_channel.received_packets.insert(i + 32, ());
        }
        receive_channel.received_packets.insert(0, ());

        let acks = receive_channel.generate_acks();
        assert_eq!(AckData { ack: 63, ack_bits: 0b11111111111111111111111111111111u32 }, acks[0]);
        assert_eq!(AckData { ack: 31, ack_bits: 0b10000000000000000000000000000000u32 }, acks[1]);
    }
}