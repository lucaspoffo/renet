use crate::channel::reliable::{ReceiveChannelReliable, SendChannelReliable};
use crate::channel::unreliable::{ReceiveChannelUnreliable, SendChannelUnreliable};
use crate::channel::{ChannelConfig, DefaultChannel, SendType};
use crate::connection_stats::ConnectionStats;
use crate::error::DisconnectReason;
use crate::packet::{Packet, Payload};
use bytes::Bytes;
use octets::OctetsMut;

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub available_bytes_per_tick: u64,
    pub server_channels_config: Vec<ChannelConfig>,
    pub client_channels_config: Vec<ChannelConfig>,
}

#[derive(Debug, Clone)]
struct PacketSent {
    sent_at: Duration,
    info: PacketSentInfo,
}

#[derive(Debug, Clone)]
enum PacketSentInfo {
    // No need to track info for unreliable messages
    None,
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

#[derive(Debug)]
enum ChannelOrder {
    Reliable(u8),
    Unreliable(u8),
}

pub struct NetworkInfo {
    /// Round-trip Time
    pub rtt: f64,
    pub packet_loss: f64,
    /// Sent bytes per second.
    pub bytes_sent_per_second: f64,
    /// Received bytes per second.
    pub bytes_received_per_second: f64,
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct RenetClient {
    packet_sequence: u64,
    current_time: Duration,
    sent_packets: BTreeMap<u64, PacketSent>,
    pending_acks: Vec<Range<u64>>,
    channel_send_order: Vec<ChannelOrder>,
    send_unreliable_channels: HashMap<u8, SendChannelUnreliable>,
    receive_unreliable_channels: HashMap<u8, ReceiveChannelUnreliable>,
    send_reliable_channels: HashMap<u8, SendChannelReliable>,
    receive_reliable_channels: HashMap<u8, ReceiveChannelReliable>,
    stats: ConnectionStats,
    available_bytes_per_tick: u64,
    largest_acked_packet: u64,
    pub(crate) disconnect_reason: Option<DisconnectReason>,
    rtt: f64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            // At 60 hz, this becomes 2.4 mbps
            available_bytes_per_tick: 60 * 1024,
            server_channels_config: DefaultChannel::config(),
            client_channels_config: DefaultChannel::config(),
        }
    }
}

impl RenetClient {
    pub fn new(config: ConnectionConfig) -> Self {
        Self::from_channels(
            config.available_bytes_per_tick,
            config.client_channels_config,
            config.server_channels_config,
        )
    }

    // When creating a client from the server, the server_channels_config are used as send channels,
    // and the client_channels_config is used as recv channels.
    pub(crate) fn new_from_server(config: ConnectionConfig) -> Self {
        Self::from_channels(
            config.available_bytes_per_tick,
            config.server_channels_config,
            config.client_channels_config,
        )
    }

    fn from_channels(
        available_bytes_per_tick: u64,
        send_channels_config: Vec<ChannelConfig>,
        receive_channels_config: Vec<ChannelConfig>,
    ) -> Self {
        let mut send_unreliable_channels = HashMap::new();
        let mut send_reliable_channels = HashMap::new();
        let mut channel_send_order: Vec<ChannelOrder> = Vec::with_capacity(send_channels_config.len());
        for channel_config in send_channels_config.iter() {
            match channel_config.send_type {
                SendType::Unreliable => {
                    let channel = SendChannelUnreliable::new(channel_config.channel_id, channel_config.max_memory_usage_bytes);
                    let old = send_unreliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists send channel {}", channel_config.channel_id);

                    channel_send_order.push(ChannelOrder::Unreliable(channel_config.channel_id));
                }
                SendType::ReliableOrdered { resend_time } | SendType::ReliableUnordered { resend_time } => {
                    let channel = SendChannelReliable::new(channel_config.channel_id, resend_time, channel_config.max_memory_usage_bytes);
                    let old = send_reliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists send channel {}", channel_config.channel_id);

                    channel_send_order.push(ChannelOrder::Reliable(channel_config.channel_id));
                }
            }
        }

        let mut receive_unreliable_channels = HashMap::new();
        let mut receive_reliable_channels = HashMap::new();
        for channel_config in receive_channels_config.iter() {
            match channel_config.send_type {
                SendType::Unreliable => {
                    let channel = ReceiveChannelUnreliable::new(channel_config.channel_id, channel_config.max_memory_usage_bytes);
                    let old = receive_unreliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists receive channel {}", channel_config.channel_id);
                }
                SendType::ReliableOrdered { .. } => {
                    let channel = ReceiveChannelReliable::new(channel_config.max_memory_usage_bytes, true);
                    let old = receive_reliable_channels.insert(channel_config.channel_id, channel);
                    assert!(old.is_none(), "already exists receive channel {}", channel_config.channel_id);
                }
                SendType::ReliableUnordered { .. } => {
                    let channel = ReceiveChannelReliable::new(channel_config.max_memory_usage_bytes, false);
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
            channel_send_order,
            send_unreliable_channels,
            receive_unreliable_channels,
            send_reliable_channels,
            receive_reliable_channels,
            stats: ConnectionStats::new(),
            rtt: 0.0,
            available_bytes_per_tick,
            largest_acked_packet: 0,
            disconnect_reason: None,
        }
    }

    pub fn rtt(&self) -> f64 {
        self.rtt
    }

    pub fn packet_loss(&self) -> f64 {
        self.stats.packet_loss()
    }

    pub fn bytes_sent_per_sec(&self) -> f64 {
        self.stats.bytes_sent_per_second(self.current_time)
    }

    pub fn bytes_received_per_sec(&self) -> f64 {
        self.stats.bytes_received_per_second(self.current_time)
    }

    pub fn network_info(&self) -> NetworkInfo {
        NetworkInfo {
            rtt: self.rtt,
            packet_loss: self.stats.packet_loss(),
            bytes_sent_per_second: self.stats.bytes_sent_per_second(self.current_time),
            bytes_received_per_second: self.stats.bytes_received_per_second(self.current_time),
        }
    }

    #[inline]
    pub fn is_disconnected(&self) -> bool {
        self.disconnect_reason.is_some()
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.disconnect_reason.is_none()
    }

    pub fn disconnect_reason(&self) -> Option<DisconnectReason> {
        self.disconnect_reason
    }

    pub fn disconnect(&mut self) {
        if self.disconnect_reason.is_some() {
            return;
        }

        self.disconnect_reason = Some(DisconnectReason::DisconnectedByClient);
    }

    // pub fn can_send_message<I: Into<u8>>(&self, channel_id: I) -> bool {
    //     let channel = self.send_channels.get(&channel_id.into()).expect("invalid channel id");
    //     channel.can_send_message()
    // }

    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        if self.is_disconnected() {
            return;
        }

        let channel_id = channel_id.into();
        if let Some(reliable_channel) = self.send_reliable_channels.get_mut(&channel_id) {
            if let Err(error) = reliable_channel.send_message(message.into()) {
                self.disconnect_reason = Some(DisconnectReason::SendChannelError { channel_id, error });
            }
        } else if let Some(unreliable_channel) = self.send_unreliable_channels.get_mut(&channel_id) {
            unreliable_channel.send_message(message.into());
        } else {
            panic!("Tried to send message to invalid channel {channel_id}");
        }
    }

    pub fn receive_message<I: Into<u8>>(&mut self, channel_id: I) -> Option<Bytes> {
        if self.is_disconnected() {
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
        self.stats.update(self.current_time);

        // Discard lost packets
        let mut lost_packets: Vec<u64> = Vec::new();
        for (&sequence, sent_packet) in self.sent_packets.range(0..self.largest_acked_packet + 1) {
            const DISCARD_AFTER: Duration = Duration::from_secs(3);
            if self.current_time - sent_packet.sent_at >= DISCARD_AFTER {
                lost_packets.push(sequence);
            } else {
                // If the current packet is not lost, the next ones will not be lost
                // since all the next packets will be sent after this one.
                break;
            }
        }

        for sequence in lost_packets.iter() {
            self.sent_packets.remove(sequence);
        }
    }

    pub fn process_packet(&mut self, packet: &[u8]) {
        if self.is_disconnected() {
            return;
        }

        self.stats.received_packet(packet.len() as u64);
        let mut octets = octets::Octets::with_slice(packet);
        let Ok(packet) = Packet::from_bytes(&mut octets) else {
            self.disconnect_reason = Some(DisconnectReason::PacketDeserialization);
            return;
        };

        match packet {
            Packet::SmallReliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_reliable_channels.get_mut(&channel_id) else {
                    self.disconnect_reason = Some(DisconnectReason::ReceivedInvalidChannelId(channel_id));
                    return;
                };

                for (message_id, message) in messages {
                    if let Err(error) = channel.process_message(message, message_id) {
                        self.disconnect_reason = Some(DisconnectReason::ReceiveChannelError { channel_id, error });
                        return;
                    }
                }
            }
            Packet::SmallUnreliable {
                packet_sequence,
                channel_id,
                messages,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_unreliable_channels.get_mut(&channel_id) else {
                    self.disconnect_reason = Some(DisconnectReason::ReceivedInvalidChannelId(channel_id));
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
                    self.disconnect_reason = Some(DisconnectReason::ReceivedInvalidChannelId(channel_id));
                    return;
                };

                if let Err(error) = channel.process_slice(slice) {
                    self.disconnect_reason = Some(DisconnectReason::ReceiveChannelError { channel_id, error });
                }
            }
            Packet::UnreliableSlice {
                packet_sequence,
                channel_id,
                slice,
            } => {
                self.add_pending_ack(packet_sequence);
                let Some(channel) = self.receive_unreliable_channels.get_mut(&channel_id) else {
                    self.disconnect_reason = Some(DisconnectReason::ReceivedInvalidChannelId(channel_id));
                    return;
                };

                if let Err(error) = channel.process_slice(slice, self.current_time) {
                    self.disconnect_reason = Some(DisconnectReason::ReceiveChannelError { channel_id, error });
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
                    self.stats.acked_packet(sent_packet.sent_at, self.current_time);

                    // Update rtt
                    let rtt = (self.current_time - sent_packet.sent_at).as_secs_f64();
                    if self.rtt < f64::EPSILON {
                        self.rtt = rtt;
                    } else {
                        self.rtt = self.rtt * 0.875 + rtt * 0.125;
                    }

                    match sent_packet.info {
                        PacketSentInfo::ReliableMessages { channel_id, message_ids } => {
                            let reliable_channel = self.send_reliable_channels.get_mut(&channel_id).unwrap();
                            for message_id in message_ids {
                                reliable_channel.process_message_ack(message_id);
                            }
                        }
                        PacketSentInfo::ReliableSliceMessage {
                            channel_id,
                            message_id,
                            slice_index,
                        } => {
                            let reliable_channel = self.send_reliable_channels.get_mut(&channel_id).unwrap();
                            reliable_channel.process_slice_message_ack(message_id, slice_index);
                        }
                        PacketSentInfo::Ack { largest_acked_packet } => {
                            self.acked_largest(largest_acked_packet);
                        }
                        PacketSentInfo::None => {}
                    }
                }
            }
        }
    }

    pub fn get_packets_to_send(&mut self) -> Vec<Payload> {
        let mut packets: Vec<Packet> = vec![];
        if self.is_disconnected() {
            return vec![];
        }

        let mut available_bytes = self.available_bytes_per_tick;
        for order in self.channel_send_order.iter() {
            match order {
                ChannelOrder::Reliable(channel_id) => {
                    let channel = self.send_reliable_channels.get_mut(channel_id).unwrap();
                    packets.append(&mut channel.get_packets_to_send(&mut self.packet_sequence, &mut available_bytes, self.current_time));
                }
                ChannelOrder::Unreliable(channel_id) => {
                    let channel = self.send_unreliable_channels.get_mut(channel_id).unwrap();
                    packets.append(&mut channel.get_packets_to_send(&mut self.packet_sequence, &mut available_bytes));
                }
            }
        }

        if !self.pending_acks.is_empty() {
            let ack_packet = Packet::Ack {
                packet_sequence: self.packet_sequence,
                ack_ranges: self.pending_acks.clone(),
            };
            self.packet_sequence += 1;
            packets.push(ack_packet);
        }

        let sent_at = self.current_time;
        for packet in packets.iter() {
            match packet {
                Packet::SmallReliable {
                    packet_sequence,
                    channel_id,
                    messages,
                } => {
                    self.sent_packets.insert(
                        *packet_sequence,
                        PacketSent {
                            sent_at,
                            info: PacketSentInfo::ReliableMessages {
                                channel_id: *channel_id,
                                message_ids: messages.iter().map(|(id, _)| *id).collect(),
                            },
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
                        PacketSent {
                            sent_at,
                            info: PacketSentInfo::ReliableSliceMessage {
                                channel_id: *channel_id,
                                message_id: slice.message_id,
                                slice_index: slice.slice_index,
                            },
                        },
                    );
                }
                Packet::SmallUnreliable { packet_sequence, .. } => {
                    self.sent_packets.insert(
                        *packet_sequence,
                        PacketSent {
                            sent_at,
                            info: PacketSentInfo::None,
                        },
                    );
                }
                Packet::UnreliableSlice { packet_sequence, .. } => {
                    self.sent_packets.insert(
                        *packet_sequence,
                        PacketSent {
                            sent_at,
                            info: PacketSentInfo::None,
                        },
                    );
                }
                Packet::Ack {
                    packet_sequence,
                    ack_ranges,
                } => {
                    let last_range = ack_ranges.last().unwrap();
                    let largest_acked_packet = last_range.end - 1;
                    self.sent_packets.insert(
                        *packet_sequence,
                        PacketSent {
                            sent_at,
                            info: PacketSentInfo::Ack { largest_acked_packet },
                        },
                    );
                }
            }
        }

        let mut buffer = [0u8; 1400];
        let mut serialized_packets = Vec::with_capacity(packets.len());
        let mut bytes_sent: u64 = 0;
        for packet in packets {
            let mut oct = OctetsMut::with_slice(&mut buffer);
            let Ok(len) = packet.to_bytes(&mut oct) else {
                self.disconnect_reason = Some(DisconnectReason::PacketSerialization);
                return vec![];
            };
            bytes_sent += len as u64;
            serialized_packets.push(buffer[..len].to_vec());
        }

        self.stats.sent_packets(serialized_packets.len() as u64, bytes_sent);

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
        if self.largest_acked_packet > largest_ack {
            return;
        }

        self.largest_acked_packet = largest_ack;
        while !self.pending_acks.is_empty() {
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
        let mut connection = RenetClient::new(ConnectionConfig::default());
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
        let mut connection = RenetClient::new(ConnectionConfig::default());
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

    #[test]
    fn discard_old_packets() {
        let mut connection = RenetClient::new(ConnectionConfig::default());
        let message: Bytes = vec![5; 5].into();
        connection.send_message(0, message);

        connection.get_packets_to_send();
        assert_eq!(connection.sent_packets.len(), 1);

        connection.advance_time(Duration::from_secs(1));
        assert_eq!(connection.sent_packets.len(), 1);

        connection.advance_time(Duration::from_secs(4));
        assert_eq!(connection.sent_packets.len(), 0);
    }
}
