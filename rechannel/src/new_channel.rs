use crate::error::ChannelError;
use crate::packet::AckData;
use crate::sequence_buffer::{sequence_greater_than, sequence_less_than, SequenceBuffer};

use bytes::Bytes;

use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Clone)]
struct Message {
    message_type: MessageType,
    payload: Bytes,
}

// Remove serde
// Dont use reassembly_fragment, just send packet with 1200 bytes, slice when 1000+ bytes and send in parts
// Remove sequenced channel, add example how to it with a SequencedMessage<T> struct

const SLICE_SIZE: usize = 400;

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
}

struct Packet {
    sequence: u16,
    ack_data: AckData,
    channel_id: u8,
    messages: Vec<Message>,
}

struct PacketSent {
    time: Duration,
    ack: bool,
}

struct ReliableMessage {}

#[derive(Debug, Clone)]
enum ReliableSend {
    Full {
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

struct SlicedMessage {
    num_slices: usize,
    current_slice_id: usize,
    num_acked_slices: usize,
    acked: Vec<bool>,
    last_sent: Vec<Option<Duration>>,
}

struct SendChannel {
    packets_sent: SequenceBuffer<PacketSent>,
    unreliable_messages: VecDeque<Vec<u8>>,
    reliable_messages_send: SequenceBuffer<ReliableSend>,
    send_message_id: u16,
    oldest_unacked_message_id: u16,
    resend_time: Duration,
    error: Option<ChannelError>,
}

struct ReceiveChannel {
    unreliable_messages: VecDeque<Vec<u8>>,
    reliable_messages_received: SequenceBuffer<ReceiveReliableMessage>,
    next_message_id: u16,
    error: Option<ChannelError>,
}

#[derive(Debug, Clone)]
enum ReceiveReliableMessage {
    Received {
        payload: Vec<u8>,
    },
    ReceivingSliced {
        num_slices: usize,
        num_received_slices: usize,
        received: Vec<bool>,
        chunk_data: Vec<u8>,
    },
}

impl SendChannel {
    fn generate_messages_to_send(&mut self, available_bytes: &mut u64, current_time: Duration) -> Vec<Message> {
        let mut packets = vec![];

        if self.error.is_some() {
            return packets;
        }

        // Check if has messages to send
        if self.oldest_unacked_message_id == self.send_message_id {
            return packets;
        }

        for i in 0..self.reliable_messages_send.size() {
            let message_id = self.oldest_unacked_message_id.wrapping_add(i as u16);

            if let Some(message_send) = self.reliable_messages_send.get_mut(message_id) {
                match message_send {
                    ReliableSend::Full { payload, last_sent } => {
                        let send_message = match last_sent {
                            None => true,
                            Some(last_sent) => *last_sent + self.resend_time >= current_time,
                        };
                        if !send_message {
                            continue;
                        }

                        // No bytes available to send this message
                        // TODO: make Message::serialized_size_with_payload
                        let serialized_size = payload.len() as u64;
                        if *available_bytes < serialized_size {
                            continue;
                        }

                        *available_bytes -= serialized_size;
                        *last_sent = Some(current_time);
                        let message = Message {
                            payload: payload.clone(),
                            message_type: MessageType::Reliable { message_id },
                        };

                        packets.push(message);
                    }
                    ReliableSend::Sliced {
                        payload,
                        num_slices,
                        acked,
                        last_sent,
                        ..
                    } => {
                        for slice_index in 0..*num_slices {
                            // TODO: add size of the message header
                            if *available_bytes < SLICE_SIZE as u64 {
                                break;
                            }

                            // Skip slice if already acked
                            if acked[slice_index] {
                                continue;
                            }

                            // Check if is it the time to send this message
                            match last_sent[slice_index] {
                                Some(last_sent) if last_sent + self.resend_time < current_time => continue,
                                _ => {}
                            }

                            let start = slice_index * SLICE_SIZE;
                            // Last slice does not have fixed size
                            let end = if slice_index == *num_slices - 1 { payload.len() } else { (slice_index + 1) * SLICE_SIZE };

                            let sliced_payload: Bytes = payload.slice(start..end);

                            // TODO: make Message::serialized_size_with_payload
                            let serialized_size = sliced_payload.len() as u64;
                            *available_bytes -= serialized_size;
                            last_sent[slice_index] = Some(current_time);

                            let message = Message {
                                payload: sliced_payload,
                                message_type: MessageType::ReliableSliced {
                                    message_id,
                                    slice_index: slice_index as u16,
                                    num_slices: *num_slices as u16,
                                },
                            };

                            packets.push(message);
                        }
                    }
                }
            }
        }

        packets
    }
}

impl ReceiveChannel {
    fn process_message(&mut self, message: Message) {
        match message.message_type {
            MessageType::Unreliable => {}
            MessageType::Reliable { message_id } => {
                if sequence_less_than(message_id, self.next_message_id) {
                    // Discard old message
                    return;
                }

                let max_message_id = self.next_message_id.wrapping_add(self.reliable_messages_received.size() as u16 - 1);
                if sequence_greater_than(message_id, max_message_id) {
                    // Out of space to to add messages
                    self.error = Some(ChannelError::ReliableChannelOutOfSync);
                }

                if !self.reliable_messages_received.exists(message_id) {
                    let new_message = ReceiveReliableMessage::Received {
                        payload: message.payload.into(),
                    };
                    self.reliable_messages_received.insert(message_id, new_message);
                }
            }
            MessageType::ReliableSliced {
                message_id,
                slice_index,
                num_slices,
            } => {
                if sequence_less_than(message_id, self.next_message_id) {
                    // Discard old message
                    return;
                }

                let max_message_id = self.next_message_id.wrapping_add(self.reliable_messages_received.size() as u16 - 1);
                if sequence_greater_than(message_id, max_message_id) {
                    // Out of space to to add messages
                    self.error = Some(ChannelError::ReliableChannelOutOfSync);
                }

                self.reliable_messages_received
                    .get_or_insert_with(message_id, || ReceiveReliableMessage::ReceivingSliced {
                        num_slices: num_slices as usize,
                        num_received_slices: 0,
                        received: vec![false; num_slices as usize],
                        chunk_data: vec![0; num_slices as usize * SLICE_SIZE],
                    });

                let Some(ReceiveReliableMessage::ReceivingSliced { num_slices, num_received_slices, received, chunk_data }) = self.reliable_messages_received.get_mut(message_id) else {
                    return;
                };

                // if message.num_slices != *num_slices as u32 {
                //     error!(
                //         "Invalid number of slices for SliceMessage, got {}, expected {}.",
                //         message.num_slices, num_slices,
                //     );
                //     return Err(ChannelError::InvalidSliceMessage);
                // }

                let slice_id = slice_index as usize;
                let is_last_slice = slice_id == *num_slices - 1;
                if is_last_slice {
                    if message.payload.len() > SLICE_SIZE {
                        log::error!(
                            "Invalid last slice_size for SliceMessage, got {}, expected less than {}.",
                            message.payload.len(),
                            SLICE_SIZE,
                        );
                        self.error = Some(ChannelError::InvalidSliceMessage);
                    }
                } else if message.payload.len() != SLICE_SIZE {
                    log::error!(
                        "Invalid slice_size for SliceMessage, got {}, expected {}.",
                        message.payload.len(),
                        SLICE_SIZE
                    );
                    self.error = Some(ChannelError::InvalidSliceMessage);
                }

                if !received[slice_id] {
                    received[slice_id] = true;
                    *num_received_slices += 1;

                    if is_last_slice {
                        let len = (*num_slices - 1) * SLICE_SIZE + message.payload.len();
                        chunk_data.resize(len, 0);
                    }

                    let start = slice_id * SLICE_SIZE;
                    let end = if slice_id == *num_slices - 1 {
                        (*num_slices - 1) * SLICE_SIZE + message.payload.len()
                    } else {
                        (slice_id + 1) * SLICE_SIZE
                    };

                    chunk_data[start..end].copy_from_slice(&message.payload);
                    log::trace!(
                        "Received slice {} from message {}. ({}/{})",
                        slice_id,
                        message_id,
                        num_received_slices,
                        num_slices
                    );
                }

                if *num_received_slices == *num_slices {
                    log::trace!("Received all slices for message {}.", message_id);
                    let payload = std::mem::take(chunk_data);
                    let new_message = ReceiveReliableMessage::Received { payload };
                    self.reliable_messages_received.insert(message_id, new_message);
                }
            }
        }
    }
}

impl Message {}
