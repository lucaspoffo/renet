use crate::channel::{ChannelConfig, DefaultChannel, ReceiveChannel, SendChannel};
use crate::error::{DisconnectionReason, RechannelError};
use crate::network_info::{BandwidthInfo, NetworkInfo};
use crate::packet::{Packet, Payload};

use crate::reassembly_fragment::{build_fragments, FragmentConfig, ReassemblyFragment};
use crate::sequence_buffer::SequenceBuffer;
use crate::timer::Timer;
use crate::transport::ClientTransport;

use bincode::Options;
use bytes::Bytes;
use log::error;

use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
struct SentPacket {
    time: Duration,
    ack: bool,
}

#[derive(Debug)]
enum ConnectionState {
    Connected,
    Disconnected { reason: DisconnectionReason },
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub max_packet_size: u64,
    pub sent_packets_buffer_size: usize,
    pub received_packets_buffer_size: usize,
    pub rtt_smoothing_factor: f32,
    pub packet_loss_smoothing_factor: f32,
    pub bandwidth_smoothing_factor: f32,
    pub heartbeat_time: Duration,
    pub fragment_config: FragmentConfig,
    pub send_channels_config: Vec<ChannelConfig>,
    pub receive_channels_config: Vec<ChannelConfig>,
}

#[derive(Debug)]
pub struct RemoteConnection {
    state: ConnectionState,
    sequence: u16,
    send_channels: HashMap<u8, Box<dyn SendChannel + Send + Sync + 'static>>,
    receive_channels: HashMap<u8, Box<dyn ReceiveChannel + Send + Sync + 'static>>,
    heartbeat_timer: Timer,
    config: ConnectionConfig,
    reassembly_buffer: SequenceBuffer<ReassemblyFragment>,
    sent_buffer: SequenceBuffer<SentPacket>,
    received_buffer: SequenceBuffer<()>,
    current_time: Duration,
    bandwidth_info: BandwidthInfo,
    rtt: f32,
    packet_loss: f32,
    acks: Vec<u16>,
}

impl SentPacket {
    fn new(time: Duration) -> Self {
        Self { time, ack: false }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_packet_size: 16 * 1024,
            sent_packets_buffer_size: 256,
            received_packets_buffer_size: 256,
            rtt_smoothing_factor: 0.005,
            packet_loss_smoothing_factor: 0.1,
            bandwidth_smoothing_factor: 0.1,
            heartbeat_time: Duration::from_millis(100),
            fragment_config: FragmentConfig::default(),
            send_channels_config: DefaultChannel::config(),
            receive_channels_config: DefaultChannel::config(),
        }
    }
}

impl RemoteConnection {
    pub fn new(config: ConnectionConfig) -> Self {
        config.fragment_config.assert_can_fragment_packet_with_size(config.max_packet_size);

        let current_time = Duration::ZERO;
        let heartbeat_timer = Timer::new(current_time, config.heartbeat_time);
        let reassembly_buffer = SequenceBuffer::with_capacity(config.fragment_config.reassembly_buffer_size);
        let sent_buffer = SequenceBuffer::with_capacity(config.sent_packets_buffer_size);
        let received_buffer = SequenceBuffer::with_capacity(config.received_packets_buffer_size);

        let mut send_channels = HashMap::new();
        for channel_config in config.send_channels_config.iter() {
            let (send_channel, _) = channel_config.new_channels();
            let channel_id = channel_config.channel_id();
            let old_channel = send_channels.insert(channel_id, send_channel);
            assert!(old_channel.is_none(), "already exists send channel with id {}", channel_id);
        }

        let mut receive_channels = HashMap::new();
        for channel_config in config.receive_channels_config.iter() {
            let (_, receive_channel) = channel_config.new_channels();
            let channel_id = channel_config.channel_id();
            let old_channel = receive_channels.insert(channel_id, receive_channel);
            assert!(old_channel.is_none(), "already exists receive channel with id {}", channel_id);
        }

        Self {
            state: ConnectionState::Connected,
            send_channels,
            receive_channels,
            heartbeat_timer,
            sequence: 0,
            reassembly_buffer,
            sent_buffer,
            received_buffer,
            current_time,
            bandwidth_info: BandwidthInfo::new(config.bandwidth_smoothing_factor),
            rtt: 0.0,
            packet_loss: 0.0,
            config,
            acks: vec![],
        }
    }

    pub fn network_info(&self) -> NetworkInfo {
        NetworkInfo {
            sent_kbps: self.bandwidth_info.sent_kbps,
            received_kbps: self.bandwidth_info.received_kbps,
            rtt: self.rtt,
            packet_loss: self.packet_loss,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, ConnectionState::Connected)
    }

    pub fn disconnected(&self) -> Option<DisconnectionReason> {
        match self.state {
            ConnectionState::Disconnected { reason } => Some(reason),
            _ => None,
        }
    }

    pub fn disconnect(&mut self) {
        if matches!(self.state, ConnectionState::Disconnected { .. }) {
            error!("Trying to disconnect an already disconnected client.");
            return;
        }

        self.state = ConnectionState::Disconnected {
            reason: DisconnectionReason::DisconnectedByClient,
        };
    }

    pub fn can_send_message<I: Into<u8>>(&self, channel_id: I) -> bool {
        let channel = self.send_channels.get(&channel_id.into()).expect("invalid channel id");
        channel.can_send_message()
    }

    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        let channel = self.send_channels.get_mut(&channel_id.into()).expect("invalid channel id");
        channel.send_message(message.into(), self.current_time);
    }

    pub fn receive_message<I: Into<u8>>(&mut self, channel_id: I) -> Option<Payload> {
        let channel = self.receive_channels.get_mut(&channel_id.into()).expect("invalid channel id");
        channel.receive_message()
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), RechannelError> {
        self.current_time += duration;

        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }

        for (&channel_id, send_channel) in self.send_channels.iter() {
            if let Some(error) = send_channel.error() {
                let reason = DisconnectionReason::SendChannelError { channel_id, error };
                self.state = ConnectionState::Disconnected { reason };
                return Err(RechannelError::ClientDisconnected(reason));
            }
        }

        for (&channel_id, receive_channel) in self.receive_channels.iter() {
            if let Some(error) = receive_channel.error() {
                let reason = DisconnectionReason::ReceiveChannelError { channel_id, error };
                self.state = ConnectionState::Disconnected { reason };
                return Err(RechannelError::ClientDisconnected(reason));
            }
        }

        for ack in self.acks.drain(..) {
            for channel in self.send_channels.values_mut() {
                channel.process_ack(ack);
            }
        }

        self.update_packet_loss();
        self.bandwidth_info.update_metrics();

        Ok(())
    }

    pub fn process_packet(&mut self, packet: &[u8]) -> Result<(), RechannelError> {
        self.bandwidth_info.received_packet(self.current_time, packet.len());

        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }

        let packet: Packet = bincode::options().deserialize(packet)?;

        let channels_packet_data = match packet {
            Packet::Normal {
                sequence,
                ack_data,
                channels_packet_data,
            } => {
                self.received_buffer.insert(sequence, ());
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                channels_packet_data
            }
            Packet::Fragment {
                sequence,
                ack_data,
                fragment_data,
            } => {
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);

                let packet = self.reassembly_buffer.handle_fragment(
                    sequence,
                    fragment_data,
                    self.config.max_packet_size,
                    &self.config.fragment_config,
                )?;
                match packet {
                    None => return Ok(()),
                    Some(packet) => {
                        // Only consider the packet received when the fragment is completed
                        self.received_buffer.insert(sequence, ());
                        packet
                    }
                }
            }
            Packet::Heartbeat { ack_data } => {
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                return Ok(());
            }
            Packet::Disconnect { reason } => {
                self.state = ConnectionState::Disconnected { reason };
                return Ok(());
            }
        };

        for channel_packet_data in channels_packet_data.into_iter() {
            let receive_channel = match self.receive_channels.get_mut(&channel_packet_data.channel_id) {
                Some(c) => c,
                None => {
                    let reason = DisconnectionReason::InvalidChannelId(channel_packet_data.channel_id);
                    self.state = ConnectionState::Disconnected { reason };
                    return Err(RechannelError::ClientDisconnected(reason));
                }
            };

            receive_channel.process_messages(channel_packet_data.messages);
        }

        Ok(())
    }

    pub fn get_packets_to_send(&mut self) -> Result<Vec<Payload>, RechannelError> {
        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }

        let sequence = self.sequence;
        // Aproximated header size for the packet
        const HEADER_SIZE: u64 = 20;
        let mut available_bytes = self.config.max_packet_size - HEADER_SIZE;
        let mut channels_packet_data = vec![];
        for send_channel in self.send_channels.values_mut() {
            if let Some(channel_packet_data) = send_channel.get_messages_to_send(available_bytes, sequence, self.current_time) {
                available_bytes -= bincode::options().serialized_size(&channel_packet_data)?;
                channels_packet_data.push(channel_packet_data);
            }
        }

        let packets = if !channels_packet_data.is_empty() {
            self.sequence = self.sequence.wrapping_add(1);
            let packet_size = bincode::options().serialized_size(&channels_packet_data)?;
            let ack_data = self.received_buffer.ack_data();

            let sent_packet = SentPacket::new(self.current_time);
            self.sent_buffer.insert(sequence, sent_packet);

            let packets: Vec<Payload> = if packet_size > self.config.fragment_config.fragment_above {
                build_fragments(channels_packet_data, sequence, ack_data, &self.config.fragment_config)?
            } else {
                let packet = Packet::Normal {
                    sequence,
                    ack_data,
                    channels_packet_data,
                };
                let packet = bincode::options().serialize(&packet)?;
                vec![packet]
            };

            self.heartbeat_timer.reset(self.current_time);
            packets
        } else if self.heartbeat_timer.is_finished(self.current_time) {
            let ack_data = self.received_buffer.ack_data();
            let packet = Packet::Heartbeat { ack_data };
            let packet = bincode::options().serialize(&packet)?;

            self.heartbeat_timer.reset(self.current_time);
            vec![packet]
        } else {
            return Ok(vec![]);
        };

        for packet in &packets {
            self.bandwidth_info.sent_packet(self.current_time, packet.len());
        }

        Ok(packets)
    }

    fn update_acket_packets(&mut self, ack: u16, mut ack_bits: u32) {
        for i in 0..32 {
            if ack_bits & 1 != 0 {
                let ack_sequence = ack.wrapping_sub(i);
                if let Some(ref mut sent_packet) = self.sent_buffer.get_mut(ack_sequence) {
                    if !sent_packet.ack {
                        self.acks.push(ack_sequence);
                        sent_packet.ack = true;

                        // Update RTT
                        let rtt = (self.current_time - sent_packet.time).as_secs_f32() * 1000.;

                        if self.rtt == 0.0 || self.rtt < f32::EPSILON || self.config.rtt_smoothing_factor == 0.0 {
                            self.rtt = rtt;
                        } else {
                            self.rtt += (rtt - self.rtt) * self.config.rtt_smoothing_factor;
                        }
                    }
                }
            }
            ack_bits >>= 1;
        }
    }

    fn update_packet_loss(&mut self) {
        let sample_size = self.config.sent_packets_buffer_size;
        let base_sequence = self.sent_buffer.sequence().wrapping_sub(sample_size as u16);

        let mut packets_dropped = 0;
        let mut packets_sent = 0;
        for i in 0..sample_size {
            if let Some(sent_packet) = self.sent_buffer.get(base_sequence.wrapping_add(i as u16)) {
                packets_sent += 1;
                let secs_since_sent = (self.current_time - sent_packet.time).as_secs_f32();
                if !sent_packet.ack && secs_since_sent > self.rtt * 1.5 {
                    packets_dropped += 1;
                }
            }
        }

        if packets_sent == 0 {
            return;
        }

        let packet_loss = packets_dropped as f32 / packets_sent as f32;
        dbg!(packets_dropped, packets_sent);
        if self.packet_loss == 0.0 || self.packet_loss < f32::EPSILON || self.config.packet_loss_smoothing_factor == 0.0 {
            self.packet_loss = packet_loss;
        } else {
            self.packet_loss += (packet_loss - self.packet_loss) * self.config.packet_loss_smoothing_factor;
        }
    }

    pub fn send_packets(&mut self, transport: &mut dyn ClientTransport) {
        let packets = match self.get_packets_to_send() {
            Ok(p) => p,
            Err(e) => {
                log::error!("Failed to get packets to send: {}", e);
                return;
            },
        };

        for packet in packets.iter() {
           transport.send(packet);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::packet::AckData;

    use super::*;

    #[test]
    fn round_time_trip() {
        let mut connection = RemoteConnection::new(ConnectionConfig::default());

        let message: Bytes = vec![1, 2, 3].into();
        let mut ack_data = AckData { ack: 0, ack_bits: 1 };
        for _ in 0..16 {
            connection.send_message(1, message.clone());
            assert!(!connection.get_packets_to_send().unwrap().is_empty());

            connection.update(Duration::from_millis(100)).unwrap();
            connection.update_acket_packets(ack_data.ack, ack_data.ack_bits);

            ack_data.ack += 1;
        }

        assert_eq!(connection.network_info().rtt, 100.);
    }

    #[test]
    fn packet_loss() {
        let mut connection = RemoteConnection::new(ConnectionConfig {
            packet_loss_smoothing_factor: 0.0,
            ..Default::default()
        });

        let message: Bytes = vec![1, 2, 3].into();
        let mut ack_data = AckData { ack: 0, ack_bits: 1 };
        for i in 0..32 {
            connection.send_message(1, message.clone());
            assert!(!connection.get_packets_to_send().unwrap().is_empty());

            // 50% packet loss
            if i % 2 == 0 {
                connection.update_acket_packets(ack_data.ack, ack_data.ack_bits);
            }
            connection.update(Duration::from_millis(100)).unwrap();
            ack_data.ack += 1;
        }

        connection.update_packet_loss();
        assert_eq!(connection.network_info().packet_loss, 0.5);
    }

    #[test]
    fn confirm_only_completed_fragmented_packet() {
        let config = ConnectionConfig::default();
        let mut connection = RemoteConnection::new(config);
        let message = vec![7u8; 2500];
        connection.send_message(0, message.clone());

        let packets = connection.get_packets_to_send().unwrap();
        assert!(packets.len() > 1);
        for packet in packets.iter() {
            assert!(!connection.received_buffer.exists(0));
            connection.process_packet(packet).unwrap();
        }
        // After all fragments are received it should be considered received
        assert!(connection.received_buffer.exists(0));

        let received_message = connection.receive_message(0).unwrap();
        assert_eq!(message, received_message);
    }
}
