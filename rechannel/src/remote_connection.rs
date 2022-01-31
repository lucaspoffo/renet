use crate::channel::{Channel, ChannelConfig};
use crate::error::{DisconnectionReason, RechannelError};
use crate::packet::{ChannelMessages, HeartBeat, Normal, Packet, Payload};

use crate::reassembly_fragment::{build_fragments, FragmentConfig, ReassemblyFragment};
use crate::sequence_buffer::SequenceBuffer;
use crate::timer::Timer;

use bincode::Options;
use log::{debug, error};

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct SentPacket {
    // TODO: replace with Duration
    time: Instant,
    ack: bool,
    size_bytes: usize,
}

#[derive(Debug, Clone)]
struct ReceivedPacket {
    time: Instant,
    size_bytes: usize,
}

#[derive(Debug)]
enum ConnectionState {
    Connected,
    Disconnected { reason: DisconnectionReason },
}

#[derive(Debug, Default)]
pub struct NetworkInfo {
    pub rtt: f64,
    pub sent_bandwidth_kbps: f64,
    pub received_bandwidth_kbps: f64,
    pub packet_loss: f64,
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub max_packet_size: u64,
    pub sent_packets_buffer_size: usize,
    pub received_packets_buffer_size: usize,
    pub measure_smoothing_factor: f64,
    pub timeout_duration: Duration,
    pub heartbeat_time: Duration,
    pub channels_config: Vec<ChannelConfig>,
    pub fragment_config: FragmentConfig,
}

#[derive(Debug)]
pub struct RemoteConnection {
    state: ConnectionState,
    sequence: u16,
    channels: HashMap<u8, Channel>,
    heartbeat_timer: Timer,
    timeout_timer: Timer,
    pub config: ConnectionConfig,
    reassembly_buffer: SequenceBuffer<ReassemblyFragment>,
    sent_buffer: SequenceBuffer<SentPacket>,
    received_buffer: SequenceBuffer<ReceivedPacket>,
    network_info: NetworkInfo,
    acks: Vec<u16>,
}

impl SentPacket {
    fn new(time: Instant, size_bytes: usize) -> Self {
        Self {
            time,
            size_bytes,
            ack: false,
        }
    }
}

impl ReceivedPacket {
    fn new(time: Instant, size_bytes: usize) -> Self {
        Self { time, size_bytes }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_packet_size: 16 * 1024,
            sent_packets_buffer_size: 256,
            received_packets_buffer_size: 256,
            measure_smoothing_factor: 0.1,
            timeout_duration: Duration::from_secs(5),
            heartbeat_time: Duration::from_millis(200),
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
    pub fn new(config: ConnectionConfig) -> Self {
        let timeout_timer = Timer::new(config.timeout_duration);
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
            timeout_timer,
            heartbeat_timer,
            sequence: 0,
            reassembly_buffer,
            sent_buffer,
            received_buffer,
            config,
            network_info: NetworkInfo::default(),
            acks: vec![],
        }
    }

    pub fn network_info(&self) -> &NetworkInfo {
        &self.network_info
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

    pub fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), RechannelError> {
        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }
        let channel = self.channels.get_mut(&channel_id).expect("invalid channel");
        if let Err(e) = channel.send_message(message) {
            if let RechannelError::ClientDisconnected(reason) = e {
                self.state = ConnectionState::Disconnected { reason };
            }
            return Err(e);
        }
        Ok(())
    }

    pub fn receive_message(&mut self, channel_id: u8) -> Option<Payload> {
        let channel = self.channels.get_mut(&channel_id).expect("invalid channel");
        channel.receive_message()
    }

    pub fn advance_time(&mut self, duration: Duration) {
        self.heartbeat_timer.advance(duration);
        self.timeout_timer.advance(duration);
        for channel in self.channels.values_mut() {
            channel.advance_time(duration);
        }
    }

    pub fn update(&mut self) -> Result<(), RechannelError> {
        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }

        if self.timeout_timer.is_finished() {
            return self.set_disconnected(DisconnectionReason::TimedOut);
        }

        for (channel_id, channel) in self.channels.iter() {
            if let Channel::Reliable(reliable_channel) = channel {
                if reliable_channel.out_of_sync() {
                    let reason = DisconnectionReason::ReliableChannelOutOfSync(*channel_id);
                    self.state = ConnectionState::Disconnected { reason };
                    return Err(RechannelError::ClientDisconnected(reason));
                }
            }
        }

        for ack in self.acks.drain(..) {
            for channel in self.channels.values_mut() {
                channel.process_ack(ack);
            }
        }
        self.update_sent_bandwidth();
        self.update_received_bandwidth();

        Ok(())
    }

    pub fn process_packet(&mut self, packet: &[u8]) -> Result<(), RechannelError> {
        if let Some(reason) = self.disconnected() {
            return Err(RechannelError::ClientDisconnected(reason));
        }

        self.timeout_timer.reset();
        let received_bytes = packet.len();
        let packet: Packet = bincode::options().deserialize(packet)?;

        let channels_messages = match packet {
            Packet::Normal(Normal {
                sequence,
                ack_data,
                channel_messages,
            }) => {
                let received_packet = ReceivedPacket::new(Instant::now(), received_bytes);
                self.received_buffer.insert(sequence, received_packet);
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                channel_messages
            }
            Packet::Fragment(fragment) => {
                if let Some(received_packet) = self.received_buffer.get_mut(fragment.sequence) {
                    received_packet.size_bytes += received_bytes;
                } else {
                    let received_packet = ReceivedPacket::new(Instant::now(), received_bytes);
                    self.received_buffer.insert(fragment.sequence, received_packet);
                }

                self.update_acket_packets(fragment.ack_data.ack, fragment.ack_data.ack_bits);

                let payload =
                    self.reassembly_buffer
                        .handle_fragment(fragment, self.config.max_packet_size, &self.config.fragment_config)?;
                match payload {
                    None => return Ok(()),
                    Some(p) => p,
                }
            }
            Packet::Heartbeat(HeartBeat { ack_data }) => {
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                return Ok(());
            }
            Packet::Disconnect(reason) => {
                self.state = ConnectionState::Disconnected { reason };
                // TODO: should return Err?
                return Ok(());
            }
        };

        for channel_data in channels_messages.reliable_channels_data.into_iter() {
            let channel = match self.channels.get_mut(&channel_data.channel_id) {
                Some(c) => c,
                None => return self.set_disconnected(DisconnectionReason::InvalidChannelId(channel_data.channel_id)),
            };
            match channel {
                Channel::Reliable(reliable_channel) => reliable_channel.process_messages(channel_data.messages),
                _ => return self.set_disconnected(DisconnectionReason::MismatchingChannelType(channel_data.channel_id)),
            }
        }

        for channel_data in channels_messages.unreliable_channels_data.into_iter() {
            let channel = match self.channels.get_mut(&channel_data.channel_id) {
                Some(c) => c,
                None => return self.set_disconnected(DisconnectionReason::InvalidChannelId(channel_data.channel_id)),
            };
            match channel {
                Channel::Unreliable(unreliable_channel) => unreliable_channel.process_messages(channel_data.messages),
                _ => return self.set_disconnected(DisconnectionReason::MismatchingChannelType(channel_data.channel_id)),
            }
        }

        for channel_data in channels_messages.block_channels_data.into_iter() {
            let channel = match self.channels.get_mut(&channel_data.channel_id) {
                Some(c) => c,
                None => return self.set_disconnected(DisconnectionReason::InvalidChannelId(channel_data.channel_id)),
            };
            match channel {
                Channel::Block(block_channel) => block_channel.process_messages(channel_data.messages),
                _ => return self.set_disconnected(DisconnectionReason::MismatchingChannelType(channel_data.channel_id)),
            }
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
        let mut reliable_channels_data = Vec::new();
        let mut unreliable_channels_data = Vec::new();
        let mut block_channels_data = Vec::new();
        for channel in self.channels.values_mut() {
            match channel {
                Channel::Reliable(reliable_channel) => {
                    if let Some(channel_data) = reliable_channel.get_messages_to_send(available_bytes, sequence)? {
                        available_bytes -= bincode::options().serialized_size(&channel_data)?;
                        reliable_channels_data.push(channel_data);
                    }
                }
                Channel::Unreliable(unreliable_channel) => {
                    if let Some(channel_data) = unreliable_channel.get_messages_to_send(available_bytes) {
                        available_bytes -= bincode::options().serialized_size(&channel_data)?;
                        unreliable_channels_data.push(channel_data);
                    }
                }
                Channel::Block(block_channel) => {
                    if let Some(channel_data) = block_channel.get_messages_to_send(available_bytes, sequence)? {
                        available_bytes -= bincode::options().serialized_size(&channel_data)?;
                        block_channels_data.push(channel_data);
                    }
                }
            }
        }

        let channel_messages = ChannelMessages {
            reliable_channels_data,
            unreliable_channels_data,
            block_channels_data,
        };

        if !channel_messages.is_empty() {
            self.sequence = self.sequence.wrapping_add(1);
            let packet_size = bincode::options().serialized_size(&channel_messages)?;
            let ack_data = self.received_buffer.ack_data();

            let sent_packet = SentPacket::new(Instant::now(), packet_size as usize);
            self.sent_buffer.insert(sequence, sent_packet);

            let packets: Vec<Payload> = if packet_size > self.config.fragment_config.fragment_above as u64 {
                build_fragments(channel_messages, sequence, ack_data, &self.config.fragment_config)?
            } else {
                let packet = Packet::Normal(Normal {
                    sequence,
                    ack_data,
                    channel_messages,
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

    pub fn set_disconnected<T>(&mut self, reason: DisconnectionReason) -> Result<T, RechannelError> {
        self.state = ConnectionState::Disconnected { reason };
        Err(RechannelError::ClientDisconnected(reason))
    }

    fn update_acket_packets(&mut self, ack: u16, ack_bits: u32) {
        let mut ack_bits = ack_bits;
        let now = Instant::now();
        for i in 0..32 {
            if ack_bits & 1 != 0 {
                let ack_sequence = ack.wrapping_sub(i);
                if let Some(ref mut sent_packet) = self.sent_buffer.get_mut(ack_sequence) {
                    if !sent_packet.ack {
                        debug!("Acked packet {}", ack_sequence);
                        self.acks.push(ack_sequence);
                        sent_packet.ack = true;
                        let rtt = (now - sent_packet.time).as_secs_f64();
                        if self.network_info.rtt == 0.0 && rtt > 0.0 || f64::abs(self.network_info.rtt - rtt) < 0.00001 {
                            self.network_info.rtt = rtt;
                        } else {
                            self.network_info.rtt += (rtt - self.network_info.rtt) * self.config.measure_smoothing_factor;
                        }
                    }
                }
            }
            ack_bits >>= 1;
        }
    }

    fn update_sent_bandwidth(&mut self) {
        let sample_size = self.config.sent_packets_buffer_size / 4;
        let base_sequence = self.sent_buffer.sequence().wrapping_sub(sample_size as u16);

        let mut packets_dropped = 0;
        let mut bytes_sent = 0;
        let mut start_time = Instant::now();
        let mut end_time = Instant::now() - Duration::from_secs(100);
        for i in 0..sample_size {
            if let Some(sent_packet) = self.sent_buffer.get(base_sequence.wrapping_add(i as u16)) {
                if sent_packet.size_bytes == 0 {
                    // Only Default Packets have size 0
                    continue;
                }
                bytes_sent += sent_packet.size_bytes;
                if sent_packet.time < start_time {
                    start_time = sent_packet.time;
                }
                if sent_packet.time > end_time {
                    end_time = sent_packet.time;
                }
                // FIXME: check against RTT to see if it has actually dropped
                if !sent_packet.ack {
                    packets_dropped += 1;
                }
            }
        }

        // Calculate packet loss
        let packet_loss = packets_dropped as f64 / sample_size as f64 * 100.0;
        if f64::abs(self.network_info.packet_loss - packet_loss) > 0.0001 {
            self.network_info.packet_loss += (packet_loss - self.network_info.packet_loss) * self.config.measure_smoothing_factor;
        } else {
            self.network_info.packet_loss = packet_loss;
        }

        // Calculate sent bandwidth
        if end_time <= start_time {
            return;
        }

        let sent_bandwidth_kbps = bytes_sent as f64 / (end_time - start_time).as_secs_f64() * 8.0 / 1000.0;
        if f64::abs(self.network_info.sent_bandwidth_kbps - sent_bandwidth_kbps) > 0.0001 {
            self.network_info.sent_bandwidth_kbps +=
                (sent_bandwidth_kbps - self.network_info.sent_bandwidth_kbps) * self.config.measure_smoothing_factor;
        } else {
            self.network_info.sent_bandwidth_kbps = sent_bandwidth_kbps;
        }
    }

    fn update_received_bandwidth(&mut self) {
        let sample_size = self.config.received_packets_buffer_size / 4;
        let base_sequence = self.received_buffer.sequence().wrapping_sub(sample_size as u16 - 1);

        let mut bytes_received = 0;
        let mut start_time = Instant::now();
        let mut end_time = Instant::now() - Duration::from_secs(100);
        for i in 0..sample_size {
            if let Some(received_packet) = self.received_buffer.get_mut(base_sequence.wrapping_add(i as u16)) {
                bytes_received += received_packet.size_bytes;
                if received_packet.time < start_time {
                    start_time = received_packet.time;
                }
                if received_packet.time > end_time {
                    end_time = received_packet.time;
                }
            }
        }

        if end_time <= start_time {
            return;
        }

        let received_bandwidth_kbps = bytes_received as f64 / (end_time - start_time).as_secs_f64() * 8.0 / 1000.0;
        if f64::abs(self.network_info.received_bandwidth_kbps - received_bandwidth_kbps) > 0.0001 {
            self.network_info.received_bandwidth_kbps +=
                (received_bandwidth_kbps - self.network_info.received_bandwidth_kbps) * self.config.measure_smoothing_factor;
        } else {
            self.network_info.received_bandwidth_kbps = received_bandwidth_kbps;
        }
    }
}
