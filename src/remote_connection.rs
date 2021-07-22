use crate::channel::Channel;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::{
    AckData, Authenticated, ChannelPacketData, HeartBeat, Message, Normal, Packet, Payload,
    Unauthenticaded,
};
use crate::protocol::{AuthenticationProtocol, SecurityService};
use crate::reassembly_fragment::{build_fragments, FragmentConfig, ReassemblyFragment};
use crate::sequence_buffer::SequenceBuffer;
use crate::Timer;

use log::{debug, error, trace};

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub type ClientId = u64;

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

pub enum ConnectionState<P: AuthenticationProtocol> {
    Connecting { protocol: P },
    Connected { security_service: P::Service },
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

pub struct RemoteConnection<P: AuthenticationProtocol> {
    client_id: ClientId,
    state: ConnectionState<P>,
    sequence: u16,
    addr: SocketAddr,
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

impl<P: AuthenticationProtocol> RemoteConnection<P> {
    pub fn new(
        client_id: ClientId,
        addr: SocketAddr,
        config: ConnectionConfig,
        protocol: P,
    ) -> Self {
        let timeout_timer = Timer::new(config.timeout_duration);
        let heartbeat_timer = Timer::new(config.heartbeat_time);
        let reassembly_buffer =
            SequenceBuffer::with_capacity(config.fragment_config.reassembly_buffer_size);
        let sent_buffer = SequenceBuffer::with_capacity(config.sent_packets_buffer_size);
        let received_buffer = SequenceBuffer::with_capacity(config.received_packets_buffer_size);
        let state = ConnectionState::Connecting { protocol };

        Self {
            client_id,
            channels: HashMap::new(),
            state,
            addr,
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

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn network_info(&self) -> &NetworkInfo {
        &self.network_info
    }

    pub fn client_id(&self) -> ClientId {
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

    pub fn is_disconnected(&self) -> bool {
        matches!(self.state, ConnectionState::Disconnected { .. })
    }

    pub fn connection_error(&self) -> Option<DisconnectionReason> {
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
        match self.state {
            ConnectionState::Connecting { ref mut protocol } => protocol.create_payload(),
            _ => Ok(None),
        }
    }

    pub fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), RenetError> {
        let channel = self
            .channels
            .get_mut(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        channel
            .send_message(message)
            .map_err(|e| RenetError::ChannelError(e))
    }

    pub fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, RenetError> {
        let channel = match self.channels.get_mut(&channel_id) {
            Some(c) => c,
            None => return Err(RenetError::InvalidChannel { channel_id }),
        };

        Ok(channel.receive_message())
    }

    pub fn update(&mut self) {
        if self.is_disconnected() {
            return;
        }

        if self.has_timed_out() {
            self.state = ConnectionState::Disconnected {
                reason: DisconnectionReason::TimedOut,
            };
            return;
        }

        match self.state {
            ConnectionState::Connecting { ref mut protocol } => {
                if protocol.is_authenticated() {
                    let security_service = protocol.build_security_interface();
                    self.state = ConnectionState::Connected { security_service };
                }
            }
            ConnectionState::Connected { .. } => {
                for ack in self.acks.drain(..) {
                    for channel in self.channels.values_mut() {
                        channel.process_ack(ack);
                    }
                }
            }
            ConnectionState::Disconnected { .. } => {
                unreachable!()
            }
        }
    }

    pub fn process_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        self.timeout_timer.reset();
        let packet: Packet = bincode::deserialize(&payload)?;

        match packet {
            Packet::Unauthenticaded(Unauthenticaded::ConnectionError(error)) => {
                self.state = ConnectionState::Disconnected {
                    reason: error.clone(),
                };
                Err(RenetError::ConnectionError(error))
            }
            Packet::Unauthenticaded(Unauthenticaded::Protocol { payload }) => {
                match self.state {
                    ConnectionState::Connecting { ref mut protocol } => {
                        protocol.read_payload(&payload)?;
                    }
                    ConnectionState::Disconnected { .. } => {
                        return Err(RenetError::ClientDisconnected);
                    }
                    ConnectionState::Connected { .. } => {
                        trace!("unauthenticaded packet discarted, client is already connected");
                        return Ok(());
                    }
                }
                Ok(())
            }
            Packet::Authenticated(Authenticated { payload }) => match self.state {
                ConnectionState::Disconnected { .. } => Err(RenetError::ClientDisconnected),
                ConnectionState::Connecting { .. } => Err(RenetError::NotAuthenticated),
                ConnectionState::Connected {
                    ref mut security_service,
                } => {
                    let payload = security_service.ss_unwrap(&payload)?;
                    let message = bincode::deserialize(&payload)?;
                    let channels_packet_data = match message {
                        Message::Normal(Normal {
                            sequence,
                            ack_data,
                            payload,
                        }) => {
                            let received_packet =
                                ReceivedPacket::new(Instant::now(), payload.len());
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
                            self.state = ConnectionState::Disconnected {
                                reason: error.clone(),
                            };
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
            },
        }
    }

    pub fn send_channels_packet_data(
        &mut self,
        channels_packet_data: Vec<ChannelPacketData>,
        socket: &UdpSocket,
    ) -> Result<(), RenetError> {
        let security_service = match self.state {
            ConnectionState::Connected {
                ref mut security_service,
            } => security_service,
            ConnectionState::Connecting { .. } => return Err(RenetError::NotAuthenticated),
            ConnectionState::Disconnected { .. } => return Err(RenetError::ClientDisconnected),
        };

        let packet_size = bincode::serialized_size(&channels_packet_data)?;
        let sequence = self.sequence;
        self.sequence += 1;

        let (ack, ack_bits) = self.received_buffer.ack_bits();

        let sent_packet = SentPacket::new(Instant::now(), channels_packet_data.len());
        self.sent_buffer.insert(sequence, sent_packet);

        let payload = if packet_size > self.config.fragment_config.fragment_above as u64 {
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

        for packet in payload.iter() {
            let packet = security_service.ss_wrap(packet)?;
            let packet = Packet::Authenticated(Authenticated { payload: packet });
            let packet = bincode::serialize(&packet)?;
            socket.send_to(&packet, self.addr)?;
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

    pub fn send_packets(&mut self, socket: &UdpSocket) -> Result<(), RenetError> {
        match self.state {
            ConnectionState::Connected {
                ref mut security_service,
            } => {
                let sequence = self.sequence;
                let mut channels_packet_data: Vec<ChannelPacketData> = vec![];
                for (channel_id, channel) in self.channels.iter_mut() {
                    let messages =
                        channel.get_messages_to_send(self.config.max_packet_size as u32, sequence);
                    if let Some(messages) = messages {
                        debug!("Sending {} messages.", messages.len());
                        let packet_data = ChannelPacketData::new(messages, *channel_id);
                        channels_packet_data.push(packet_data);
                    }
                }

                if !channels_packet_data.is_empty() {
                    self.send_channels_packet_data(channels_packet_data, socket)?;
                    self.heartbeat_timer.reset();
                } else if self.heartbeat_timer.is_finished() {
                    let (ack, ack_bits) = self.received_buffer.ack_bits();
                    let message = Message::Heartbeat(HeartBeat {
                        ack_data: AckData { ack, ack_bits },
                    });

                    let message = bincode::serialize(&message)?;

                    let payload = security_service.ss_wrap(&message)?;
                    let packet = Packet::Authenticated(Authenticated { payload });
                    let packet = bincode::serialize(&packet)?;
                    socket.send_to(&packet, self.addr)?;
                    self.heartbeat_timer.reset();
                }
            }
            ConnectionState::Connecting { ref mut protocol } => {
                if let Some(payload) = protocol.create_payload()? {
                    let packet = Packet::Unauthenticaded(Unauthenticaded::Protocol { payload });
                    let packet = bincode::serialize(&packet)?;
                    socket.send_to(&packet, self.addr)?;
                    self.heartbeat_timer.reset();
                }
            }
            ConnectionState::Disconnected { .. } => {
                return Err(RenetError::ClientDisconnected);
            }
        }
        Ok(())
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
        let payload = match self.state {
            ConnectionState::Connected {
                ref mut security_service,
            } => {
                let message = Message::Disconnect(reason);
                let message = bincode::serialize(&message)?;
                let payload = security_service.ss_wrap(&message)?;
                let packet = Packet::Authenticated(Authenticated { payload });

                bincode::serialize(&packet)?
            }
            _ => {
                let packet = Packet::Unauthenticaded(Unauthenticaded::ConnectionError(
                    DisconnectionReason::DisconnectedByServer,
                ));

                bincode::serialize(&packet)?
            }
        };

        socket.send_to(&payload, self.addr)?;
        Ok(())
    }
}
