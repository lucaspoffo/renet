use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    another_channel::{Packet, Slice, SmallMessage},
    error::ChannelError,
};

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

struct SendChannel {
    channel_id: u8,
    unreliable_messages: VecDeque<Bytes>,
    unacked_messages: BTreeMap<u64, UnackedMessage>,
    next_reliable_message_id: u64,
    unreliable_message_id: u64,
    oldest_unacked_message_id: u64,
    resend_time: Duration,
    max_unacked_bytes: usize,
    unacked_bytes: usize,
    error: Option<ChannelError>,
}

pub enum SendError {
    ReachedBytesLimit,
}

const SLICE_SIZE: usize = 1200;

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

impl SendChannel {
    fn get_messages_to_send(&mut self, packet_sequence: &mut u64, current_time: Duration) -> Vec<Packet> {
        let mut packets: Vec<Packet> = vec![];

        let mut normal_messages: Vec<SmallMessage> = vec![];
        let mut normal_messages_bytes = 0;

        let mut generate_normal_packet = |messages: &mut Vec<SmallMessage>, packets: &mut Vec<Packet>, packet_sequence: &mut u64| {
            let messages = std::mem::take(messages);

            packets.push(Packet::Normal {
                packet_sequence: packet_sequence.clone(),
                channel_id: self.channel_id,
                messages,
            });
            *packet_sequence += 1;
            normal_messages_bytes = 0;
        };

        let should_send = |last_sent: Option<Duration>| {
            if let Some(last_sent) = last_sent {
                return last_sent - current_time < self.resend_time;
            }
            true
        };

        for (&message_id, unacked_message) in self.unacked_messages.iter_mut() {
            match unacked_message {
                UnackedMessage::Small { payload, last_sent } => {
                    if !should_send(*last_sent) {
                        continue;
                    }

                    let message = SmallMessage::Reliable {
                        message_id,
                        payload: payload.clone(),
                    };

                    // Generate packet with small messages if you cannot fit
                    // if normal_messages_bytes + message.serialized_size() > MAX_PACKET_SIZE {
                    // generate_normal_packet();
                    // }

                    // normal_messages_bytes += message.serilized_size();
                    normal_messages.push(message);
                    *last_sent = Some(current_time);

                    continue;
                }
                UnackedMessage::Sliced {
                    payload,
                    num_slices,
                    num_acked_slices,
                    acked,
                    last_sent,
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

    fn send_unreliable_message(&mut self, message: Bytes) -> Result<(), SendError> {
        // TODO: maybe check if there is too much bytes inflight and just drop the message
        self.unreliable_messages.push_back(message);

        Ok(())
    }

    fn send_reliable_message(&mut self, message: Bytes) -> Result<(), SendError> {
        if self.unacked_bytes + message.len() > self.max_unacked_bytes {
            return Err(SendError::ReachedBytesLimit);
        }

        self.unacked_bytes += message.len();
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

        Ok(())
    }

    fn process_reliable_message_ack(&mut self, message_id: u64) {
        if self.unacked_messages.contains_key(&message_id) {
            let unacked_message = self.unacked_messages.remove(&message_id).unwrap();
            let UnackedMessage::Small { payload, .. } = unacked_message else {
                unreachable!("called ack on small message but found sliced");
            };
            self.unacked_bytes -= payload.len();
        }
    }

    fn process_reliable_slice_message_ack(&mut self, message_id: u64, slice_index: usize) {
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
            self.unacked_bytes -= payload.len();
            self.unacked_messages.remove(&message_id);
        }
    }
}

struct SendChannelUnreliableSequenced {
    channel_id: u8,
    message_id: u64,
    unreliable_messages: VecDeque<Bytes>,
    error: Option<ChannelError>,
}

impl SendChannelUnreliableSequenced {
    fn get_messages_to_send(&mut self, packet_sequence: &mut u64, current_time: Duration) -> Vec<Packet> {
        let mut packets: Vec<Packet> = vec![];

        let mut small_messages: Vec<(u64, Bytes)> = vec![];
        let mut small_messages_bytes = 0;

        let mut generate_normal_packet = |messages: &mut Vec<(u64, Bytes)>, packets: &mut Vec<Packet>, packet_sequence: &mut u64| {
            let messages = std::mem::take(messages);

            packets.push(Packet::SmallUnreliableSequenced {
                packet_sequence: packet_sequence.clone(),
                channel_id: self.channel_id,
                messages,
            });

            *packet_sequence += 1;
            small_messages_bytes = 0;
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
                    generate_normal_packet(&mut small_messages, &mut packets, packet_sequence);
                }

                small_messages_bytes += unreliable_message.len();
                small_messages.push((self.message_id, unreliable_message));
                self.message_id += 1;
            }
        }

        // Generate final packet for remaining small messages
        if !small_messages.is_empty() {
            generate_normal_packet(&mut small_messages, &mut packets, packet_sequence);
        }

        packets
    }

    fn send_message(&mut self, message: Bytes) -> Result<(), SendError> {
        // TODO: maybe check if there is too much bytes inflight and just drop the message
        self.unreliable_messages.push_back(message);

        Ok(())
    }
}

struct SendChannelUnreliable {
    channel_id: u8,
    unreliable_messages: VecDeque<Bytes>,
    sliced_message_id: u64,
    error: Option<ChannelError>,
}

impl SendChannelUnreliable {
    fn get_messages_to_send(&mut self, packet_sequence: &mut u64, current_time: Duration) -> Vec<Packet> {
        let mut packets: Vec<Packet> = vec![];

        let mut small_messages: Vec<Bytes> = vec![];
        let mut small_messages_bytes = 0;

        let mut generate_normal_packet = |messages: &mut Vec<Bytes>, packets: &mut Vec<Packet>, packet_sequence: &mut u64| {
            let messages = std::mem::take(messages);

            packets.push(Packet::SmallUnreliable {
                packet_sequence: packet_sequence.clone(),
                channel_id: self.channel_id,
                messages,
            });
            *packet_sequence += 1;
            small_messages_bytes = 0;
        };

        while let Some(unreliable_message) = self.unreliable_messages.pop_front() {
            if unreliable_message.len() > SLICE_SIZE {
                let num_slices = unreliable_message.len() / SLICE_SIZE;
                for slice_index in 0..num_slices {
                    let start = slice_index * SLICE_SIZE;
                    let end = if slice_index == num_slices - 1 { unreliable_message.len() } else { (slice_index + 1) * SLICE_SIZE };
                    let payload = unreliable_message.slice(start..end);

                    let slice = Slice {
                        message_id: self.sliced_message_id,
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

                self.sliced_message_id += 1;
            } else {
                // FIXME: use const
                if small_messages_bytes + unreliable_message.len() > 1200 {
                    generate_normal_packet(&mut small_messages, &mut packets, packet_sequence);
                }

                small_messages_bytes += unreliable_message.len();
                small_messages.push(unreliable_message);
            }
        }

        // Generate final packet for remaining small messages
        if !small_messages.is_empty() {
            generate_normal_packet(&mut small_messages, &mut packets, packet_sequence);
        }

        packets
    }

    fn send_message(&mut self, message: Bytes) -> Result<(), SendError> {
        // TODO: maybe check if there is too much bytes inflight and just drop the message
        self.unreliable_messages.push_back(message);

        Ok(())
    }
}

struct SendChannelReliable {
    channel_id: u8,
    unacked_messages: BTreeMap<u64, UnackedMessage>,
    next_reliable_message_id: u64,
    oldest_unacked_message_id: u64,
    resend_time: Duration,
    max_unacked_bytes: usize,
    unacked_bytes: usize,
    error: Option<ChannelError>,
}

impl SendChannelReliable {
    fn get_messages_to_send(&mut self, packet_sequence: &mut u64, current_time: Duration) -> Vec<Packet> {
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
                return last_sent - current_time < self.resend_time;
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
                    normal_messages.push((message_id, *payload));
                    *last_sent = Some(current_time);

                    continue;
                }
                UnackedMessage::Sliced {
                    payload,
                    num_slices,
                    num_acked_slices,
                    acked,
                    last_sent,
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

    fn send_message(&mut self, message: Bytes) -> Result<(), SendError> {
        if self.unacked_bytes + message.len() > self.max_unacked_bytes {
            return Err(SendError::ReachedBytesLimit);
        }

        self.unacked_bytes += message.len();
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

        Ok(())
    }

    fn process_message_ack(&mut self, message_id: u64) {
        if self.unacked_messages.contains_key(&message_id) {
            let unacked_message = self.unacked_messages.remove(&message_id).unwrap();
            let UnackedMessage::Small { payload, .. } = unacked_message else {
                unreachable!("called ack on small message but found sliced");
            };
            self.unacked_bytes -= payload.len();
        }
    }

    fn process_slice_message_ack(&mut self, message_id: u64, slice_index: usize) {
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
            self.unacked_bytes -= payload.len();
            self.unacked_messages.remove(&message_id);
        }
    }
}