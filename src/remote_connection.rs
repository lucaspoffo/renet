use crate::channel::Channel;
use crate::error::{DisconnectionReason, MessageError, RenetError};
use crate::packet::{AckData, ChannelPacketData, HeartBeat, Message, Normal, Payload};
use crate::reassembly_fragment::{build_fragments, FragmentConfig, ReassemblyFragment};
use crate::sequence_buffer::SequenceBuffer;
use crate::timer::Timer;
use crate::ClientId;

use log::{debug, error, info};

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct SentPacket {
    time: Instant,
    ack: bool,
    size_bytes: usize,
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

#[derive(Debug, Clone)]
struct ReceivedPacket {
    time: Instant,
    size_bytes: usize,
}

impl ReceivedPacket {
    fn new(time: Instant, size_bytes: usize) -> Self {
        Self { time, size_bytes }
    }
}

enum ConnectionState {
    Connected,
    Disconnected { reason: DisconnectionReason },
}

#[derive(Debug)]
pub struct NetworkInfo {
    pub rtt: f64,
    pub sent_bandwidth_kbps: f64,
    pub received_bandwidth_kbps: f64,
    pub packet_loss: f64,
}

impl Default for NetworkInfo {
    fn default() -> Self {
        Self {
            // TODO: Check using duration for RTT
            rtt: 0.,
            sent_bandwidth_kbps: 0.,
            received_bandwidth_kbps: 0.,
            packet_loss: 0.,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub max_packet_size: usize,
    pub sent_packets_buffer_size: usize,
    pub received_packets_buffer_size: usize,
    pub measure_smoothing_factor: f64,
    pub timeout_duration: Duration,
    pub heartbeat_time: Duration,
    pub fragment_config: FragmentConfig,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_packet_size: 16 * 1024,
            sent_packets_buffer_size: 256,
            received_packets_buffer_size: 256,
            measure_smoothing_factor: 0.05,
            timeout_duration: Duration::from_secs(5),
            heartbeat_time: Duration::from_millis(200),
            fragment_config: FragmentConfig::default(),
        }
    }
}

pub(crate) struct RemoteConnection<C> {
    client_id: C,
    state: ConnectionState,
    sequence: u16,
    channels: HashMap<u8, Box<dyn Channel>>,
    heartbeat_timer: Timer,
    timeout_timer: Timer,
    config: ConnectionConfig,
    reassembly_buffer: SequenceBuffer<ReassemblyFragment>,
    sent_buffer: SequenceBuffer<SentPacket>,
    received_buffer: SequenceBuffer<ReceivedPacket>,
    acks: Vec<u16>,
    network_info: NetworkInfo,
}

impl<C: ClientId> RemoteConnection<C> {
    pub fn new(client_id: C, config: ConnectionConfig) -> Self {
        let timeout_timer = Timer::new(config.timeout_duration);
        let heartbeat_timer = Timer::new(config.heartbeat_time);
        let reassembly_buffer =
            SequenceBuffer::with_capacity(config.fragment_config.reassembly_buffer_size);
        let sent_buffer = SequenceBuffer::with_capacity(config.sent_packets_buffer_size);
        let received_buffer = SequenceBuffer::with_capacity(config.received_packets_buffer_size);
        let state = ConnectionState::Connected;

        Self {
            client_id,
            channels: HashMap::new(),
            state,
            timeout_timer,
            heartbeat_timer,
            sequence: 0,
            reassembly_buffer,
            sent_buffer,
            received_buffer,
            config,
            acks: vec![],
            network_info: NetworkInfo::default(),
        }
    }

    pub fn network_info(&self) -> &NetworkInfo {
        &self.network_info
    }

    pub fn client_id(&self) -> C {
        self.client_id
    }

    pub fn add_channel(&mut self, channel_id: u8, channel: Box<dyn Channel>) {
        self.channels.insert(channel_id, channel);
    }

    pub fn has_timed_out(&mut self) -> bool {
        self.timeout_timer.is_finished()
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, ConnectionState::Connected { .. })
    }

    pub fn connection_error(&self) -> Option<DisconnectionReason> {
        match self.state {
            ConnectionState::Disconnected { reason } => Some(reason),
            _ => None,
        }
    }

    pub fn disconnect(&mut self, reason: DisconnectionReason) {
        if matches!(self.state, ConnectionState::Disconnected { .. }) {
            error!("Trying to disconnect an already disconnected client.");
            return;
        }

        self.state = ConnectionState::Disconnected { reason };
    }

    pub fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), MessageError> {
        let channel = self
            .channels
            .get_mut(&channel_id)
            .ok_or(MessageError::InvalidChannel { channel_id })?;
        channel.send_message(message);
        Ok(())
    }

    pub fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, MessageError> {
        let channel = match self.channels.get_mut(&channel_id) {
            Some(c) => c,
            None => return Err(MessageError::InvalidChannel { channel_id }),
        };

        Ok(channel.receive_message())
    }

    pub fn update(&mut self) -> Result<(), RenetError> {
        if let Some(e) = self.connection_error() {
            return Err(e.into());
        }

        if self.has_timed_out() {
            let reason = DisconnectionReason::TimedOut;
            self.state = ConnectionState::Disconnected { reason };
            return Err(reason.into());
        }

        for (channel_id, channel) in self.channels.iter() {
            if let Some(e) = channel.error() {
                error!("Channel {} with error {}.", channel_id, e);
                let reason = DisconnectionReason::ChannelError {
                    channel_id: *channel_id,
                };
                self.state = ConnectionState::Disconnected { reason };
                return Err(reason.into());
            }
        }

        match self.state {
            ConnectionState::Connected { .. } => {
                for ack in self.acks.drain(..) {
                    for channel in self.channels.values_mut() {
                        channel.process_ack(ack);
                    }
                }
                self.update_network_info();
            }
            ConnectionState::Disconnected { .. } => {
                unreachable!()
            }
        }

        Ok(())
    }

    pub fn process_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        self.timeout_timer.reset();

        match self.state {
            ConnectionState::Connected => {
                let message: Message = bincode::deserialize(&payload)?;
                let channels_packet_data = match message {
                    Message::Normal(Normal {
                        sequence,
                        ack_data,
                        payload,
                    }) => {
                        let received_packet = ReceivedPacket::new(Instant::now(), payload.len());
                        self.received_buffer.insert(sequence, received_packet);
                        self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                        payload
                    }
                    Message::Fragment(fragment) => {
                        if let Some(received_packet) =
                            self.received_buffer.get_mut(fragment.sequence)
                        {
                            received_packet.size_bytes += payload.len();
                        } else {
                            let received_packet =
                                ReceivedPacket::new(Instant::now(), payload.len());
                            self.received_buffer
                                .insert(fragment.sequence, received_packet);
                        }

                        self.update_acket_packets(
                            fragment.ack_data.ack,
                            fragment.ack_data.ack_bits,
                        );

                        let payload = self
                            .reassembly_buffer
                            .handle_fragment(fragment, &self.config.fragment_config)?;
                        match payload {
                            None => return Ok(()),
                            Some(p) => p,
                        }
                    }
                    Message::Heartbeat(HeartBeat { ack_data }) => {
                        self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                        return Ok(());
                    }
                    Message::Disconnect(error) => {
                        self.state = ConnectionState::Disconnected { reason: error };
                        return Err(RenetError::ConnectionError(error));
                    }
                };

                for channel_packet_data in channels_packet_data.into_iter() {
                    let channel = match self.channels.get_mut(&channel_packet_data.channel_id) {
                        Some(c) => c,
                        None => {
                            error!(
                                "Discarted ChannelPacketData with invalid id: {:?}",
                                channel_packet_data.channel_id
                            );
                            continue;
                        }
                    };
                    channel.process_messages(channel_packet_data.messages);
                }

                Ok(())
            }
            _ => {
                info!("Discarted packet, current state is not connected.");
                Ok(())
            }
        }
    }

    fn update_acket_packets(&mut self, ack: u16, ack_bits: u32) {
        let mut ack_bits = ack_bits;
        let now = Instant::now();
        for i in 0..32 {
            if ack_bits & 1 != 0 {
                let ack_sequence = ack.wrapping_sub(i);
                if let Some(ref mut sent_packet) = self.sent_buffer.get_mut(ack_sequence) {
                    if !sent_packet.ack {
                        debug!("Acked packet {}.", ack_sequence);
                        self.acks.push(ack_sequence);
                        sent_packet.ack = true;
                        let rtt = (now - sent_packet.time).as_secs_f64();
                        if self.network_info.rtt == 0.0 && rtt > 0.0
                            || f64::abs(self.network_info.rtt - rtt) < 0.00001
                        {
                            self.network_info.rtt = rtt;
                        } else {
                            self.network_info.rtt += (rtt - self.network_info.rtt)
                                * self.config.measure_smoothing_factor;
                        }
                    }
                }
            }
            ack_bits >>= 1;
        }
    }

    pub fn get_packets_to_send(&mut self) -> Result<Vec<Payload>, RenetError> {
        match self.state {
            ConnectionState::Connected => {
                let sequence = self.sequence;
                let mut channels_packet_data: Vec<ChannelPacketData> = vec![];
                for (channel_id, channel) in self.channels.iter_mut() {
                    let messages =
                        channel.get_messages_to_send(self.config.max_packet_size as u32, sequence);
                    if !messages.is_empty() {
                        debug!(
                            "Sending {} messages from channel {}.",
                            messages.len(),
                            channel_id
                        );
                        let packet_data = ChannelPacketData::new(messages, *channel_id);
                        channels_packet_data.push(packet_data);
                    }
                }

                if !channels_packet_data.is_empty() {
                    let packet_size = bincode::serialized_size(&channels_packet_data)?;
                    let sequence = self.sequence;
                    self.sequence += 1;

                    let (ack, ack_bits) = self.received_buffer.ack_bits();

                    let sent_packet = SentPacket::new(Instant::now(), channels_packet_data.len());
                    self.sent_buffer.insert(sequence, sent_packet);

                    let payloads =
                        if packet_size > self.config.fragment_config.fragment_above as u64 {
                            debug!("Sending fragmented packet {}.", sequence);
                            build_fragments(
                                channels_packet_data,
                                sequence,
                                AckData { ack, ack_bits },
                                &self.config.fragment_config,
                            )?
                        } else {
                            debug!("Sending normal packet {}.", sequence);
                            let packet = Message::Normal(Normal {
                                payload: channels_packet_data.to_vec(),
                                sequence,
                                ack_data: AckData { ack, ack_bits },
                            });
                            let packet = bincode::serialize(&packet)?;
                            vec![packet]
                        };

                    self.heartbeat_timer.reset();
                    Ok(payloads)
                } else if self.heartbeat_timer.is_finished() {
                    let (ack, ack_bits) = self.received_buffer.ack_bits();
                    let message = Message::Heartbeat(HeartBeat {
                        ack_data: AckData { ack, ack_bits },
                    });

                    let packet = bincode::serialize(&message)?;
                    self.heartbeat_timer.reset();
                    Ok(vec![packet])
                } else {
                    Ok(vec![])
                }
            }
            ConnectionState::Disconnected { .. } => Err(RenetError::ClientDisconnected),
        }
    }

    pub fn update_network_info(&mut self) {
        self.update_sent_bandwidth();
        self.update_received_bandwidth();
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
                if !sent_packet.ack {
                    packets_dropped += 1;
                }
            }
        }

        // Calculate packet loss
        let packet_loss = packets_dropped as f64 / sample_size as f64 * 100.0;
        if f64::abs(self.network_info.packet_loss - packet_loss) > 0.0001 {
            self.network_info.packet_loss += (packet_loss - self.network_info.packet_loss)
                * self.config.measure_smoothing_factor;
        } else {
            self.network_info.packet_loss = packet_loss;
        }

        // Calculate sent bandwidth
        if end_time <= start_time {
            return;
        }

        let sent_bandwidth_kbps =
            bytes_sent as f64 / (end_time - start_time).as_secs_f64() * 8.0 / 1000.0;
        if f64::abs(self.network_info.sent_bandwidth_kbps - sent_bandwidth_kbps) > 0.0001 {
            self.network_info.sent_bandwidth_kbps += (sent_bandwidth_kbps
                - self.network_info.sent_bandwidth_kbps)
                * self.config.measure_smoothing_factor;
        } else {
            self.network_info.sent_bandwidth_kbps = sent_bandwidth_kbps;
        }
    }

    fn update_received_bandwidth(&mut self) {
        let sample_size = self.config.received_packets_buffer_size / 4;
        let base_sequence = self
            .received_buffer
            .sequence()
            .wrapping_sub(sample_size as u16)
            .wrapping_add(1);

        let mut bytes_received = 0;
        let mut start_time = Instant::now();
        let mut end_time = Instant::now() - Duration::from_secs(100);
        for i in 0..sample_size {
            if let Some(received_packet) = self
                .received_buffer
                .get_mut(base_sequence.wrapping_add(i as u16))
            {
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

        let received_bandwidth_kbps =
            bytes_received as f64 / (end_time - start_time).as_secs_f64() * 8.0 / 1000.0;
        if f64::abs(self.network_info.received_bandwidth_kbps - received_bandwidth_kbps) > 0.0001 {
            self.network_info.received_bandwidth_kbps += (received_bandwidth_kbps
                - self.network_info.received_bandwidth_kbps)
                * self.config.measure_smoothing_factor;
        } else {
            self.network_info.received_bandwidth_kbps = received_bandwidth_kbps;
        }
    }
}
