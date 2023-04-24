use crate::channels::reliable::{ReceiveChannelReliable, SendChannelReliable};
use crate::channels::unreliable::{ReceiveChannelUnreliable, SendChannelUnreliable};
use crate::channels::{ChannelConfig, SendType};
use crate::error::ConnectionError;
use crate::packet::{Packet, Payload};
use bytes::Bytes;
use octets::OctetsMut;

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::time::Duration;

const REQUEST_ACK_TIME: Duration = Duration::from_millis(333);

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub send_channels_config: Vec<ChannelConfig>,
    pub receive_channels_config: Vec<ChannelConfig>,
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

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            send_channels_config: vec![],
            receive_channels_config: vec![],
        }
    }
}

#[derive(Debug)]
pub struct RemoteConnection {
    packet_sequence: u64,
    current_time: Duration,
    sent_packets: BTreeMap<u64, PacketSent>,
    pending_acks: Vec<Range<u64>>,
    send_unreliable_channels: HashMap<u8, SendChannelUnreliable>,
    receive_unreliable_channels: HashMap<u8, ReceiveChannelUnreliable>,
    send_reliable_channels: HashMap<u8, SendChannelReliable>,
    receive_reliable_channels: HashMap<u8, ReceiveChannelReliable>,
    should_send_ack: bool,
    last_ack_received_at: Duration,
    error: Option<ConnectionError>,
    // rtt: f32,
    // packet_loss: f32,
}

impl RemoteConnection {
    pub fn new(config: ConnectionConfig) -> Self {
        let mut send_unreliable_channels = HashMap::new();
        let mut send_reliable_channels = HashMap::new();
        for channel_config in config.send_channels_config.iter() {
            match channel_config.send_type {
                SendType::Unreliable => {
                    let channel = SendChannelUnreliable::new(channel_config.channel_id, channel_config.max_memory_usage_bytes);
                    let old = send_unreliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists send channel {}", channel_config.channel_id);
                }
                SendType::ReliableOrdered { resend_time } | SendType::ReliableUnordered { resend_time } => {
                    let channel = SendChannelReliable::new(channel_config.channel_id, resend_time, channel_config.max_memory_usage_bytes);
                    let old = send_reliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists send channel {}", channel_config.channel_id);
                }
            }
        }

        let mut receive_unreliable_channels = HashMap::new();
        let mut receive_reliable_channels = HashMap::new();
        for channel_config in config.receive_channels_config.iter() {
            match channel_config.send_type {
                SendType::Unreliable => {
                    let channel = ReceiveChannelUnreliable::new(channel_config.channel_id, channel_config.max_memory_usage_bytes);
                    let old = receive_unreliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists receive channel {}", channel_config.channel_id);
                }
                SendType::ReliableOrdered { .. } => {
                    let channel = ReceiveChannelReliable::new(channel_config.channel_id, channel_config.max_memory_usage_bytes, true);
                    let old = receive_reliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists receive channel {}", channel_config.channel_id);
                }
                SendType::ReliableUnordered { .. } => {
                    let channel = ReceiveChannelReliable::new(channel_config.channel_id, channel_config.max_memory_usage_bytes, false);
                    let old = receive_reliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists receive channel {}", channel_config.channel_id);
                }
            }
        }

        Self {
            packet_sequence: 0,
            current_time: Duration::ZERO,
            sent_packets: BTreeMap::new(),
            pending_acks: Vec::new(),
            send_unreliable_channels,
            receive_unreliable_channels,
            send_reliable_channels,
            receive_reliable_channels,
            should_send_ack: false,
            last_ack_received_at: Duration::ZERO,
            error: None,
        }
    }

    // pub fn rtt(&self) -> f32 {
    //     self.rtt
    // }

    // pub fn packet_loss(&self) -> f32 {
    //     self.packet_loss
    // }

    #[inline]
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    pub fn error(&self) -> Option<ConnectionError> {
        self.error
    }

    // pub fn can_send_message<I: Into<u8>>(&self, channel_id: I) -> bool {
    //     let channel = self.send_channels.get(&channel_id.into()).expect("invalid channel id");
    //     channel.can_send_message()
    // }

    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        if self.has_error() {
            return;
        }

        let channel_id = channel_id.into();
        if let Some(reliable_channel) = self.send_reliable_channels.get_mut(&channel_id) {
            if let Err(error) = reliable_channel.send_message(message.into()) {
                self.error = Some(ConnectionError::SendChannelError { channel_id, error });
            }
        } else if let Some(unreliable_channel) = self.send_unreliable_channels.get_mut(&channel_id) {
            unreliable_channel.send_message(message.into());
        } else {
            panic!("Tried to send message to invalid channel {channel_id}");
        }
    }

    pub fn receive_message<I: Into<u8>>(&mut self, channel_id: I) -> Option<Bytes> {
        if self.has_error() {
            return None;
        }

        let channel_id = channel_id.into();
        if let Some(reliable_channel) = self.receive_reliable_channels.get_mut(&channel_id) {
            reliable_channel.receive_message()
        } else if let Some(unreliable_channel) = self.receive_unreliable_channels.get_mut(&channel_id) {
            unreliable_channel.receive_message()
        } else {
            panic!("Tried to receive message from invalid channel {channel_id}");
        }
    }

    pub fn advance_time(&mut self, duration: Duration) {
        self.current_time += duration;
    }

    pub fn process_packet(&mut self, packet: &[u8]) {
        if self.has_error() {
            return;
        }

        let mut octets = octets::Octets::with_slice(packet);
        let Ok(packet) = Packet::from_bytes(&mut octets) else {
            self.error = Some(ConnectionError::PacketDeserialization);
            return;
        };

        if packet.is_ack_eliciting() {
            self.should_send_ack = true;
        }

        match packet {
            Packet::SmallReliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_reliable_channels.get_mut(&channel_id) else {
                    self.error = Some(ConnectionError::ReceivedInvalidChannelId(channel_id));
                    return;
                };

                for (message_id, message) in messages {
                    channel.process_message(message, message_id);
                }
            }
            Packet::SmallUnreliable { channel_id, messages } => {
                let Some(channel) = self.receive_unreliable_channels.get_mut(&channel_id) else {
                    self.error = Some(ConnectionError::ReceivedInvalidChannelId(channel_id));
                    return;
                };

                for message in messages {
                    channel.process_message(message);
                }
            }
            Packet::ReliableSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_reliable_channels.get_mut(&channel_id) else {
                    self.error = Some(ConnectionError::ReceivedInvalidChannelId(channel_id));
                    return;
                };

                channel.process_slice(slice);
            }
            Packet::UnreliableSlice { channel_id, slice } => {
                let Some(channel) = self.receive_unreliable_channels.get_mut(&channel_id) else {
                    self.error = Some(ConnectionError::ReceivedInvalidChannelId(channel_id));
                    return;
                };

                channel.process_slice(slice, self.current_time);
            }
            Packet::Ack {
                packet_sequence,
                ack_ranges,
            } => {
                self.last_ack_received_at = self.current_time;
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
            Packet::RequestAck => {}
            Packet::Disconnect => {}
        }
    }

    pub fn get_packets_to_send(&mut self) -> Vec<Payload> {
        let mut packets: Vec<Packet> = vec![];
        if self.has_error() {
            return vec![];
        }

        for channel in self.send_unreliable_channels.values_mut() {
            packets.append(&mut channel.get_messages_to_send());
        }

        for channel in self.send_reliable_channels.values_mut() {
            packets.append(&mut channel.get_messages_to_send(&mut self.packet_sequence, self.current_time));
        }

        if !self.pending_acks.is_empty() && self.should_send_ack {
            self.should_send_ack = false;
            let ack_packet = Packet::Ack {
                packet_sequence: self.packet_sequence,
                ack_ranges: self.pending_acks.clone(),
            };
            self.packet_sequence += 1;
            packets.push(ack_packet);
        }

        for packet in packets.iter() {
            match packet {
                Packet::SmallReliable {
                    packet_sequence,
                    channel_id,
                    messages,
                } => {
                    self.sent_packets.insert(
                        *packet_sequence,
                        PacketSent::ReliableMessages {
                            channel_id: *channel_id,
                            message_ids: messages.iter().map(|(id, _)| *id).collect(),
                        },
                    );
                }
                Packet::ReliableSlice {
                    packet_sequence,
                    channel_id,
                    slice,
                } => {
                    self.sent_packets.insert(
                        *packet_sequence,
                        PacketSent::ReliableSliceMessage {
                            channel_id: *channel_id,
                            message_id: slice.message_id,
                            slice_index: slice.slice_index,
                        },
                    );
                }
                Packet::Ack {
                    packet_sequence,
                    ack_ranges,
                } => {
                    let last_range = ack_ranges.last().unwrap();
                    let largest_acked_packet = last_range.end - 1;
                    self.sent_packets.insert(*packet_sequence, PacketSent::Ack { largest_acked_packet });
                }
                _ => {}
            }
        }

        let has_ack_eliciting_packet = packets.iter().any(Packet::is_ack_eliciting);
        if !has_ack_eliciting_packet && !self.sent_packets.is_empty() {
            if self.last_ack_received_at + REQUEST_ACK_TIME < self.current_time {
                packets.push(Packet::RequestAck);
            }
        }

        let mut buffer = [0u8; 1400];
        let mut serialized_packets = Vec::with_capacity(packets.len());
        for packet in packets {
            let mut oct = OctetsMut::with_slice(&mut buffer);
            let Ok(len) = packet.to_bytes(&mut oct) else {
                self.error = Some(ConnectionError::PacketSerialization);
                return vec![];
            };
            serialized_packets.push(buffer[..len].to_vec());
        }

        serialized_packets
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

        // Limit to 64 pending ranges
        if self.pending_acks.len() > 64 {
            self.pending_acks.remove(0);
        }
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
    use super::*;

    #[test]
    fn pending_acks() {
        let mut connection = RemoteConnection::new(ConnectionConfig::default());
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
        let mut connection = RemoteConnection::new(ConnectionConfig::default());
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
        connection.acked_largest(10);
        assert_eq!(connection.pending_acks, vec![]);
    }
}
