use crate::channel::{Channel, ChannelConfig, ChannelNetworkInfo};
use crate::error::{DisconnectionReason, RechannelError};
use crate::packet::{HeartBeat, Normal, Packet, Payload};

use crate::reassembly_fragment::{build_fragments, FragmentConfig, ReassemblyFragment};
use crate::sequence_buffer::SequenceBuffer;
use crate::timer::Timer;

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
    pub heartbeat_time: Duration,
    pub fragment_config: FragmentConfig,
    pub channels_config: Vec<ChannelConfig>,
}

#[derive(Debug)]
pub struct RemoteConnection {
    state: ConnectionState,
    sequence: u16,
    channels: HashMap<u8, Box<dyn Channel + Send + Sync + 'static>>,
    heartbeat_timer: Timer,
    config: ConnectionConfig,
    reassembly_buffer: SequenceBuffer<ReassemblyFragment>,
    sent_buffer: SequenceBuffer<SentPacket>,
    received_buffer: SequenceBuffer<()>,
    current_time: Duration,
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
            rtt_smoothing_factor: 0.01,
            packet_loss_smoothing_factor: 0.1,
            heartbeat_time: Duration::from_millis(100),
            fragment_config: FragmentConfig::default(),
            channels_config: vec![
                ChannelConfig::Reliable(Default::default()),
                ChannelConfig::Unreliable(Default::default()),
                ChannelConfig::Block(Default::default()),
            ],
        }
    }
}

impl RemoteConnection {
    pub fn new(current_time: Duration, config: ConnectionConfig) -> Self {
        let heartbeat_timer = Timer::new(config.heartbeat_time);
        let reassembly_buffer = SequenceBuffer::with_capacity(config.fragment_config.reassembly_buffer_size);
        let sent_buffer = SequenceBuffer::with_capacity(config.sent_packets_buffer_size);
        let received_buffer = SequenceBuffer::with_capacity(config.received_packets_buffer_size);

        let mut channels = HashMap::new();
        for channel_config in config.channels_config.iter() {
            let channel = channel_config.new_channel();
            let channel_id = channel_config.channel_id();
            let old_channel = channels.insert(channel_id, channel);
            assert!(old_channel.is_none(), "already exists channel with id {}", channel_id);
        }

        Self {
            state: ConnectionState::Connected,
            channels,
            heartbeat_timer,
            sequence: 0,
            reassembly_buffer,
            sent_buffer,
            received_buffer,
            current_time,
            config,
            rtt: 0.0,
            packet_loss: 0.0,
            acks: vec![],
        }
    }

    pub fn rtt(&self) -> f32 {
        self.rtt
    }

    pub fn packet_loss(&self) -> f32 {
        self.packet_loss
    }

    pub fn channels_network_info(&self) -> Vec<(u8, ChannelNetworkInfo)> {
        self.channels
            .iter()
            .map(|(channel_id, channel)| (*channel_id, channel.channel_network_info()))
            .collect()
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

    pub fn can_send_message(&self, channel_id: u8) -> bool {
        let channel = self.channels.get(&channel_id).expect("invalid channel id");
        channel.can_send_message()
    }

    pub fn send_message(&mut self, channel_id: u8, message: Bytes) {
        let channel = self.channels.get_mut(&channel_id).expect("invalid channel id");
        channel.send_message(message);
    }

    pub fn receive_message(&mut self, channel_id: u8) -> Option<Payload> {
        let channel = self.channels.get_mut(&channel_id).expect("invalid channel id");
        channel.receive_message()
    }

    pub fn advance_time(&mut self, duration: Duration) {
        self.current_time += duration;
        self.heartbeat_timer.advance(duration);
        for channel in self.channels.values_mut() {
            channel.advance_time(duration);
        }
    }

    pub fn update(&mut self) -> Result<(), RechannelError> {
        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }

        for (&channel_id, channel) in self.channels.iter() {
            if let Some(error) = channel.error() {
                let reason = DisconnectionReason::ChannelError { channel_id, error };
                self.state = ConnectionState::Disconnected { reason };
                return Err(RechannelError::ClientDisconnected(reason));
            }
        }

        for ack in self.acks.drain(..) {
            for channel in self.channels.values_mut() {
                channel.process_ack(ack);
            }
        }

        self.update_packet_loss();

        Ok(())
    }

    pub fn process_packet(&mut self, packet: &[u8]) -> Result<(), RechannelError> {
        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }

        let packet: Packet = bincode::options().deserialize(packet)?;

        let channels_packet_data = match packet {
            Packet::Normal(Normal {
                sequence,
                ack_data,
                channels_packet_data,
            }) => {
                self.received_buffer.insert(sequence, ());
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                channels_packet_data
            }
            Packet::Fragment(fragment) => {
                if self.received_buffer.get_mut(fragment.sequence).is_none() {
                    self.received_buffer.insert(fragment.sequence, ());
                }

                self.update_acket_packets(fragment.ack_data.ack, fragment.ack_data.ack_bits);

                self.reassembly_buffer
                    .handle_fragment(fragment, self.config.max_packet_size, &self.config.fragment_config)?
            }
            Packet::Heartbeat(HeartBeat { ack_data }) => {
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                return Ok(());
            }
            Packet::Disconnect(reason) => {
                self.state = ConnectionState::Disconnected { reason };
                return Ok(());
            }
        };

        for channel_packet_data in channels_packet_data.into_iter() {
            let channel = match self.channels.get_mut(&channel_packet_data.channel_id) {
                Some(c) => c,
                None => {
                    let reason = DisconnectionReason::InvalidChannelId(channel_packet_data.channel_id);
                    self.state = ConnectionState::Disconnected { reason };
                    return Err(RechannelError::ClientDisconnected(reason));
                }
            };

            channel.process_messages(channel_packet_data.messages);
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
        for channel in self.channels.values_mut() {
            if let Some(channel_packet_data) = channel.get_messages_to_send(available_bytes, sequence) {
                available_bytes -= bincode::options().serialized_size(&channel_packet_data)?;
                channels_packet_data.push(channel_packet_data)
            }
        }

        if !channels_packet_data.is_empty() {
            self.sequence = self.sequence.wrapping_add(1);
            let packet_size = bincode::options().serialized_size(&channels_packet_data)?;
            let ack_data = self.received_buffer.ack_data();

            let sent_packet = SentPacket::new(self.current_time);
            self.sent_buffer.insert(sequence, sent_packet);

            let packets: Vec<Payload> = if packet_size > self.config.fragment_config.fragment_above as u64 {
                build_fragments(channels_packet_data, sequence, ack_data, &self.config.fragment_config)?
            } else {
                let packet = Packet::Normal(Normal {
                    sequence,
                    ack_data,
                    channels_packet_data,
                });
                let packet = bincode::options().serialize(&packet)?;
                vec![packet]
            };

            self.heartbeat_timer.reset();
            return Ok(packets);
        } else if self.heartbeat_timer.is_finished() {
            let ack_data = self.received_buffer.ack_data();
            let packet = Packet::Heartbeat(HeartBeat { ack_data });
            let packet = bincode::options().serialize(&packet)?;

            self.heartbeat_timer.reset();
            return Ok(vec![packet]);
        }

        // TODO: should we return Option<Vec>?
        Ok(vec![])
    }

    fn update_acket_packets(&mut self, ack: u16, ack_bits: u32) {
        let mut ack_bits = ack_bits;
        for i in 0..32 {
            if ack_bits & 1 != 0 {
                let ack_sequence = ack.wrapping_sub(i);
                if let Some(ref mut sent_packet) = self.sent_buffer.get_mut(ack_sequence) {
                    if !sent_packet.ack {
                        self.acks.push(ack_sequence);
                        sent_packet.ack = true;
                        let rtt = (self.current_time - sent_packet.time).as_secs_f32() * 1000.;

                        if self.rtt == 0.0 || self.rtt < f32::EPSILON {
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

        let packet_loss = packets_dropped as f32 / packets_sent as f32;
        if self.packet_loss == 0.0 || self.packet_loss < f32::EPSILON {
            self.packet_loss = packet_loss;
        } else {
            self.packet_loss += (packet_loss - self.packet_loss) * self.config.packet_loss_smoothing_factor;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::packet::AckData;

    use super::*;

    #[test]
    fn round_time_trip() {
        let mut connection = RemoteConnection::new(Duration::ZERO, ConnectionConfig::default());

        let message: Bytes = vec![1, 2, 3].into();
        let mut ack_data = AckData { ack: 0, ack_bits: 1 };
        for _ in 0..16 {
            connection.send_message(1, message.clone());
            assert!(!connection.get_packets_to_send().unwrap().is_empty());

            connection.advance_time(Duration::from_millis(100));
            connection.update_acket_packets(ack_data.ack, ack_data.ack_bits);

            ack_data.ack += 1;
        }

        assert_eq!(connection.rtt(), 100.);
    }

    #[test]
    fn packet_loss() {
        let mut connection = RemoteConnection::new(Duration::ZERO, ConnectionConfig::default());

        let message: Bytes = vec![1, 2, 3].into();
        let mut ack_data = AckData { ack: 0, ack_bits: 1 };
        for i in 0..32 {
            connection.send_message(1, message.clone());
            assert!(!connection.get_packets_to_send().unwrap().is_empty());

            // 50% packet loss
            if i % 2 == 0 {
                connection.update_acket_packets(ack_data.ack, ack_data.ack_bits);
            }
            connection.advance_time(Duration::from_millis(100));

            ack_data.ack += 1;
        }

        connection.update_packet_loss();
        assert_eq!(connection.packet_loss(), 0.5);
    }
}
