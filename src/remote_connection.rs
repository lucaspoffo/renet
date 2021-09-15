use crate::channel::reliable::{ReliableChannel, ReliableChannelConfig};
use crate::channel::unreliable::UnreliableChannel;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::{ChannelMessages, HeartBeat, Message, Normal, Payload};
use crate::protocol::AuthenticationProtocol;

use crate::channel::block::{BlockChannel, BlockChannelConfig};
use crate::reassembly_fragment::{build_fragments, FragmentConfig, ReassemblyFragment};
use crate::sequence_buffer::SequenceBuffer;
use crate::timer::Timer;

use bincode::Options;
use log::{debug, error};

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
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
    Connecting,
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
    pub max_packet_size: u64,
    pub sent_packets_buffer_size: usize,
    pub received_packets_buffer_size: usize,
    pub measure_smoothing_factor: f64,
    pub timeout_duration: Duration,
    pub heartbeat_time: Duration,
    pub fragment_config: FragmentConfig,
    pub block_channel_config: BlockChannelConfig,
    pub unreliable_channel_packet_budget: u64,
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
            block_channel_config: BlockChannelConfig::default(),
            unreliable_channel_packet_budget: 2000,
        }
    }
}

pub(crate) struct RemoteConnection<Protocol: AuthenticationProtocol> {
    client_id: Protocol::ClientId,
    protocol: Protocol,
    state: ConnectionState,
    sequence: u16,
    remote_addr: SocketAddr,
    reliable_channels: HashMap<u8, ReliableChannel>,
    block_channel: BlockChannel,
    unreliable_channel: UnreliableChannel,
    heartbeat_timer: Timer,
    timeout_timer: Timer,
    pub config: ConnectionConfig,
    reassembly_buffer: SequenceBuffer<ReassemblyFragment>,
    sent_buffer: SequenceBuffer<SentPacket>,
    received_buffer: SequenceBuffer<ReceivedPacket>,
    network_info: NetworkInfo,
    acks: Vec<u16>,
}

impl<Protocol: AuthenticationProtocol> RemoteConnection<Protocol> {
    pub fn new(
        client_id: Protocol::ClientId,
        addr: SocketAddr,
        config: ConnectionConfig,
        protocol: Protocol,
        realiable_channels_config: Vec<ReliableChannelConfig>,
    ) -> Self {
        let timeout_timer = Timer::new(config.timeout_duration);
        let heartbeat_timer = Timer::new(config.heartbeat_time);
        let reassembly_buffer =
            SequenceBuffer::with_capacity(config.fragment_config.reassembly_buffer_size);
        let sent_buffer = SequenceBuffer::with_capacity(config.sent_packets_buffer_size);
        let received_buffer = SequenceBuffer::with_capacity(config.received_packets_buffer_size);

        let block_channel = BlockChannel::new(config.block_channel_config.clone());
        let unreliable_channel = UnreliableChannel::new(config.unreliable_channel_packet_budget);

        let mut reliable_channels = HashMap::new();
        for channel_config in realiable_channels_config.into_iter() {
            let channel_id = channel_config.channel_id;
            let reliable_channel = ReliableChannel::new(channel_config);
            let old_channel = reliable_channels.insert(channel_id, reliable_channel);
            assert!(
                old_channel.is_none(),
                "found ReliableChannelConfig with duplicate channel_id"
            );
        }

        Self {
            client_id,
            protocol,
            state: ConnectionState::Connecting,
            remote_addr: addr,
            reliable_channels,
            block_channel,
            unreliable_channel,
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

    pub fn addr(&self) -> &SocketAddr {
        &self.remote_addr
    }

    pub fn network_info(&self) -> &NetworkInfo {
        &self.network_info
    }

    pub fn client_id(&self) -> Protocol::ClientId {
        self.client_id
    }

    pub fn has_timed_out(&mut self) -> bool {
        self.timeout_timer.is_finished()
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

    pub fn disconnect(&mut self, socket: &UdpSocket) {
        if matches!(self.state, ConnectionState::Disconnected { .. }) {
            error!("Trying to disconnect an already disconnected client.");
            return;
        }

        if let Err(e) =
            self.send_disconnect_packet(socket, DisconnectionReason::DisconnectedByClient)
        {
            error!("Failed to send disconnect packet: {}", e);
        }

        self.state = ConnectionState::Disconnected {
            reason: DisconnectionReason::DisconnectedByClient,
        };
    }

    pub fn create_protocol_payload(&mut self) -> Result<Option<Vec<u8>>, RenetError> {
        self.protocol
            .create_payload()
            .map_err(RenetError::AuthenticationError)
    }

    pub fn send_reliable_message(&mut self, channel_id: u8, message: Payload) {
        let channel = self
            .reliable_channels
            .get_mut(&channel_id)
            .expect("channel must exist to send message");

        channel.send_message(message);
    }

    pub fn send_unreliable_message(&mut self, message: Payload) {
        self.unreliable_channel.send_message(message);
    }

    pub fn send_block_message(&mut self, message: Payload) {
        self.block_channel.send_message(message);
    }

    pub fn receive_message(&mut self) -> Option<Payload> {
        for reliable_channel in self.reliable_channels.values_mut() {
            let message = reliable_channel.receive_message();
            if message.is_some() {
                return message;
            }
        }

        let unreliable_message = self.unreliable_channel.receive_message();
        if unreliable_message.is_some() {
            return unreliable_message;
        }

        let block_message = self.block_channel.receive_message();
        if block_message.is_some() {
            return block_message;
        }

        None
    }

    pub fn update(&mut self) -> Result<(), RenetError> {
        if let Some(reason) = self.disconnected() {
            return Err(reason.into());
        }

        if self.has_timed_out() {
            let reason = DisconnectionReason::TimedOut;
            self.state = ConnectionState::Disconnected { reason };
            return Err(reason.into());
        }

        if let Some(reason) = self.protocol.disconnected() {
            self.state = ConnectionState::Disconnected { reason };
            return Err(reason.into());
        }

        for (channel_id, channel) in self.reliable_channels.iter() {
            if let Some(e) = channel.error() {
                error!(
                    "Reliable Channel {} disconnected with error {}.",
                    channel_id, e
                );
                let reason = DisconnectionReason::ReliableChannelOutOfSync {
                    channel_id: *channel_id,
                };
                self.state = ConnectionState::Disconnected { reason };
                return Err(reason.into());
            }
        }

        match self.state {
            ConnectionState::Connecting => {
                if self.protocol.is_authenticated() {
                    self.state = ConnectionState::Connected;
                }
            }
            ConnectionState::Connected => {
                for ack in self.acks.drain(..) {
                    for channel in self.reliable_channels.values_mut() {
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
        if let Some(reason) = self.disconnected() {
            return Err(reason.into());
        }

        self.timeout_timer.reset();

        let packet = match self.protocol.process(payload) {
            Some(p) => p,
            None => return Ok(()),
        };

        let message: Message = bincode::options().deserialize(&packet)?;
        let received_bytes = payload.len();
        let channels_messages = match message {
            Message::Normal(Normal {
                sequence,
                ack_data,
                channel_messages,
            }) => {
                let received_packet = ReceivedPacket::new(Instant::now(), received_bytes);
                self.received_buffer.insert(sequence, received_packet);
                self.update_acket_packets(ack_data.ack, ack_data.ack_bits);
                channel_messages
            }
            Message::Fragment(fragment) => {
                if let Some(received_packet) = self.received_buffer.get_mut(fragment.sequence) {
                    received_packet.size_bytes += received_bytes;
                } else {
                    let received_packet = ReceivedPacket::new(Instant::now(), payload.len());
                    self.received_buffer
                        .insert(fragment.sequence, received_packet);
                }

                self.update_acket_packets(fragment.ack_data.ack, fragment.ack_data.ack_bits);

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

        self.unreliable_channel
            .process_messages(channels_messages.unreliable_messages);

        self.block_channel
            .process_slice_messages(channels_messages.slice_messages);

        for channel_data in channels_messages.reliable_channels_data.into_iter() {
            let channel = match self.reliable_channels.get_mut(&channel_data.channel_id) {
                Some(c) => c,
                None => {
                    error!(
                        "Discarted ReliableChannelData with invalid id {}",
                        channel_data.channel_id
                    );
                    continue;
                }
            };
            channel.process_messages(channel_data.messages);
        }

        Ok(())
    }

    pub fn send_packets(&mut self, socket: &UdpSocket) -> Result<(), RenetError> {
        if let Some(reason) = self.disconnected() {
            return Err(reason.into());
        }

        if let Some(packet) = self
            .protocol
            .create_payload()
            .map_err(RenetError::AuthenticationError)?
        {
            socket.send_to(&packet, self.remote_addr)?;
        }

        if self.is_connected() {
            let sequence = self.sequence;

            let mut available_bytes = self.config.max_packet_size;
            let mut reliable_channels_data = Vec::new();
            for reliable_channel in self.reliable_channels.values_mut() {
                if let Some(channel_data) =
                    reliable_channel.get_messages_to_send(available_bytes, sequence)?
                {
                    available_bytes -= bincode::options().serialized_size(&channel_data)?;
                    reliable_channels_data.push(channel_data);
                }
            }

            let unreliable_messages = self
                .unreliable_channel
                .get_messages_to_send(available_bytes);

            available_bytes -= bincode::options().serialized_size(&unreliable_messages)?;

            let slice_messages = self
                .block_channel
                .get_messages_to_send(available_bytes, sequence)?;

            let channel_messages = ChannelMessages {
                slice_messages,
                unreliable_messages,
                reliable_channels_data,
            };

            if !channel_messages.is_empty() {
                self.sequence = self.sequence.wrapping_add(1);
                let packet_size = bincode::options().serialized_size(&channel_messages)?;
                let ack_data = self.received_buffer.ack_data();

                let sent_packet = SentPacket::new(Instant::now(), packet_size as usize);
                self.sent_buffer.insert(sequence, sent_packet);

                let packets = if packet_size > self.config.fragment_config.fragment_above as u64 {
                    build_fragments(
                        channel_messages,
                        sequence,
                        ack_data,
                        &self.config.fragment_config,
                    )?
                } else {
                    let message = Message::Normal(Normal {
                        sequence,
                        ack_data,
                        channel_messages,
                    });
                    let packet = bincode::options().serialize(&message)?;
                    vec![packet]
                };

                for packet in packets.into_iter() {
                    let packet = self
                        .protocol
                        .wrap(&packet)
                        .map_err(RenetError::AuthenticationError)?;
                    socket.send_to(&packet, self.remote_addr)?;
                }
                self.heartbeat_timer.reset();
            } else if self.heartbeat_timer.is_finished() {
                let ack_data = self.received_buffer.ack_data();
                let message = Message::Heartbeat(HeartBeat { ack_data });

                let packet = bincode::options().serialize(&message)?;

                let packet = self
                    .protocol
                    .wrap(&packet)
                    .map_err(RenetError::AuthenticationError)?;
                socket.send_to(&packet, self.remote_addr)?;

                self.heartbeat_timer.reset();
            }
        }

        Ok(())
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

    pub fn send_disconnect_packet(
        &mut self,
        socket: &UdpSocket,
        reason: DisconnectionReason,
    ) -> Result<(), RenetError> {
        let message = Message::Disconnect(reason);
        let message = bincode::options().serialize(&message)?;
        let packet = self
            .protocol
            .wrap(&message)
            .map_err(RenetError::AuthenticationError)?;

        socket.send_to(&packet, self.remote_addr)?;
        Ok(())
    }

    pub fn try_send_disconnect_packet(&mut self, socket: &UdpSocket, reason: DisconnectionReason) {
        if let Err(e) = self.send_disconnect_packet(socket, reason) {
            error!("Error trying to send disconnect packet: {}", e);
        }
    }
}
