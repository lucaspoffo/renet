use self::packet::{
    FragmentHeader, HeaderParser, PacketHeader, PacketType, FRAGMENT_MAX_COUNT, FRAGMENT_MAX_SIZE,
};
use self::sequence_buffer::SequenceBuffer;
use async_std::net::UdpSocket;
use error::{RenetError, Result};
use futures::future::try_join_all;
use log::trace;
use std::io::{Cursor, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

mod error;
mod packet;
mod sequence_buffer;

#[derive(Clone)]
struct ReassemblyFragment {
    sequence: u16,
    num_fragments_received: usize,
    num_fragments_total: usize,
    buffer: Vec<u8>,
    fragments_received: [bool; FRAGMENT_MAX_COUNT],
}

impl Default for ReassemblyFragment {
    fn default() -> Self {
        Self {
            sequence: 0,
            num_fragments_received: 0,
            num_fragments_total: 0,
            buffer: Vec::with_capacity(0),
            fragments_received: [false; FRAGMENT_MAX_COUNT],
        }
    }
}

impl ReassemblyFragment {
    pub fn new(sequence: u16, num_fragments_total: usize) -> Self {
        let len = num_fragments_total * FRAGMENT_MAX_SIZE;
        let mut buffer = Vec::with_capacity(len);
        buffer.resize(len, 0u8);

        Self {
            sequence,
            num_fragments_received: 0,
            num_fragments_total,
            buffer,
            fragments_received: [false; FRAGMENT_MAX_COUNT],
        }
    }
}

pub struct Config {
    max_packet_size: usize,
    max_fragments: usize,
    fragment_above: usize,
    fragment_size: usize,
    fragment_reassembly_buffer_size: usize,
    sent_packets_buffer_size: usize,
    received_packets_buffer_size: usize,
    bandwidth_smoothing_factor: f64,
    rtt_smoothing_factor: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_packet_size: 16 * 1024,
            max_fragments: 32,
            fragment_above: 1024,
            fragment_size: 1024,
            fragment_reassembly_buffer_size: 256,
            sent_packets_buffer_size: 256,
            received_packets_buffer_size: 256,
            bandwidth_smoothing_factor: 0.1,
            rtt_smoothing_factor: 0.0025,
        }
    }
}

#[derive(Debug, Clone)]
struct SentPacket {
    time: Instant,
    ack: bool,
    /// Packet size in bytes
    size: usize,
}

impl Default for SentPacket {
    fn default() -> Self {
        Self {
            size: 0,
            ack: false,
            time: Instant::now(),
        }
    }
}

impl SentPacket {
    fn new(time: Instant, size: usize) -> Self {
        Self {
            time,
            size,
            ack: false,
        }
    }
}

#[derive(Debug, Clone)]
struct ReceivedPacket {
    time: Instant,
    /// Packet size in bytes
    size: usize,
}

impl Default for ReceivedPacket {
    fn default() -> Self {
        Self {
            size: 0,
            time: Instant::now(),
        }
    }
}

impl ReceivedPacket {
    fn new(time: Instant, size: usize) -> Self {
        Self { time, size }
    }
}

pub struct Endpoint {
    rtt: f64,
    config: Config,
    sequence: AtomicU16,
    reassembly_buffer: SequenceBuffer<ReassemblyFragment>,
    sent_buffer: SequenceBuffer<SentPacket>,
    received_buffer: SequenceBuffer<ReceivedPacket>,
    socket: UdpSocket,
    sent_bandwidth_kbps: f64,
    received_bandwidth_kbps: f64,
}

impl Endpoint {
    pub fn new(config: Config, socket: UdpSocket) -> Self {
        Self {
            rtt: 0.0,
            sequence: AtomicU16::new(0),
            reassembly_buffer: SequenceBuffer::with_capacity(
                config.fragment_reassembly_buffer_size,
            ),
            sent_buffer: SequenceBuffer::with_capacity(config.sent_packets_buffer_size),
            received_buffer: SequenceBuffer::with_capacity(config.received_packets_buffer_size),
            config,
            socket,
            sent_bandwidth_kbps: 0.0,
            received_bandwidth_kbps: 0.0,
        }
    }

    pub fn sent_bandwidth_kbps(&self) -> f64 {
        self.sent_bandwidth_kbps
    }

    pub fn received_bandwidth_kbps(&self) -> f64 {
        self.received_bandwidth_kbps
    }

    pub fn rtt(&self) -> f64 {
        self.rtt
    }

    pub async fn send_to(&mut self, payload: &[u8], addrs: SocketAddr) -> Result<()> {
        if payload.len() > self.config.max_packet_size {
            return Err(RenetError::MaximumPacketSizeExceeded);
        }
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let (ack, ack_bits) = self.received_buffer.ack_bits();
        // TODO: add header size
        let sent_packet = SentPacket::new(Instant::now(), payload.len());
        self.sent_buffer.insert(sequence, sent_packet);
        if payload.len() > self.config.fragment_above {
            // Fragment packet
            let fragments = build_fragments(payload, sequence, ack, ack_bits, &self.config)?;
            let futures = fragments.iter().map(|f| self.socket.send_to(&f, addrs));
            try_join_all(futures).await?;
        } else {
            // Normal packet
            let packet = build_normal_packet(payload, sequence, ack, ack_bits)?;
            self.socket.send_to(&packet, addrs).await?;
        }

        Ok(())
    }

    pub async fn recv_from(&mut self, buf: &mut [u8]) -> Result<Option<(Vec<u8>, SocketAddr)>> {
        let (n, addrs) = self.socket.recv_from(buf).await?;
        trace!("Received {} bytes from {}.", n, addrs);
        let mut payload = &buf[..n];
        if payload[0] == PacketType::Packet as u8 {
            let header = PacketHeader::parse(&mut payload)?;
            // Received packet to buffer
            let received_packet = ReceivedPacket::new(Instant::now(), payload.len());
            self.received_buffer
                .insert(header.sequence, received_packet);
            self.update_acket_packets(header.ack, header.ack_bits);
            let payload = &payload[header.size()..];
            return Ok(Some((payload.into(), addrs)));
        } else {
            let fragment_header = FragmentHeader::parse(&mut payload)?;

            if let Some(received_packet) = self.received_buffer.get_mut(fragment_header.sequence) {
                received_packet.size += payload.len();
            } else {
                let received_packet = ReceivedPacket::new(Instant::now(), payload.len());
                self.received_buffer
                    .insert(fragment_header.sequence, received_packet);
            }

            if let Some(ref packet_header) = fragment_header.packet_header {
                self.update_acket_packets(packet_header.ack, packet_header.ack_bits)
            }

            let payload = &payload[fragment_header.size()..];

            let payload = self
                .reassembly_buffer
                .handle_fragment(fragment_header, payload, &self.config)?;
            if let Some(payload) = payload {
                return Ok(Some((payload, addrs)));
            }
            return Ok(None);
        }
    }

    pub fn update_sent_bandwidth(&mut self) {
        let sample_size = self.config.sent_packets_buffer_size / 2;
        let base_sequence = self
            .sent_buffer
            .sequence()
            .wrapping_sub(self.config.sent_packets_buffer_size as u16)
            .wrapping_add(1);
        let mut bytes_sent = 0;
        let mut start_time = Instant::now();
        let mut end_time = Instant::now() - Duration::from_secs(100);
        for i in 0..sample_size {
            if let Some(sent_packet) = self
                .sent_buffer
                .get_mut(base_sequence.wrapping_add(i as u16))
            {
                bytes_sent += sent_packet.size;
                if sent_packet.time < start_time {
                    start_time = sent_packet.time;
                }
                if sent_packet.time > end_time {
                    end_time = sent_packet.time;
                }
            }
        }

        if end_time <= start_time {
            return;
        }

        let sent_bandwidth_kbps =
            bytes_sent as f64 / (end_time - start_time).as_secs_f64() * 8.0 / 1000.0;
        if f64::abs(self.sent_bandwidth_kbps - sent_bandwidth_kbps) > 0.0001 {
            self.sent_bandwidth_kbps += (sent_bandwidth_kbps - self.sent_bandwidth_kbps)
                * self.config.bandwidth_smoothing_factor;
        } else {
            self.sent_bandwidth_kbps = sent_bandwidth_kbps;
        }
    }

    pub fn update_received_bandwidth(&mut self) {
        let sample_size = self.config.received_packets_buffer_size / 2;
        let base_sequence = self
            .received_buffer
            .sequence()
            .wrapping_sub(self.config.received_packets_buffer_size as u16)
            .wrapping_add(1);

        let mut bytes_received = 0;
        let mut start_time = Instant::now();
        let mut end_time = Instant::now() - Duration::from_secs(100);
        for i in 0..sample_size {
            if let Some(received_packet) = self
                .received_buffer
                .get_mut(base_sequence.wrapping_add(i as u16))
            {
                bytes_received += received_packet.size;
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
        if f64::abs(self.received_bandwidth_kbps - received_bandwidth_kbps) > 0.0001 {
            self.received_bandwidth_kbps += (received_bandwidth_kbps
                - self.received_bandwidth_kbps)
                * self.config.bandwidth_smoothing_factor;
        } else {
            self.received_bandwidth_kbps = received_bandwidth_kbps;
        }
    }
    
    fn update_acket_packets(&mut self, ack: u16, ack_bits: u32) {
        let mut ack_bits = ack_bits;
        // TODO: should we put the now time inside the endpoint?
        let now = Instant::now();
        for i in 0..32 {
            if ack_bits & 1 != 0 {
                let ack_sequence = ack.wrapping_sub(i);
                if let Some(ref mut sent_packet) = self.sent_buffer.get_mut(ack_sequence) {
                    if !sent_packet.ack {
                        sent_packet.ack = true;
                        let rtt = (now - sent_packet.time).as_secs_f64();
                        if self.rtt == 0.0 && rtt > 0.0 || f64::abs(self.rtt - rtt) < 0.00001 {
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
}

pub fn build_normal_packet(
    payload: &[u8],
    sequence: u16,
    ack: u16,
    ack_bits: u32,
) -> Result<Vec<u8>> {
    let header = PacketHeader {
        sequence,
        ack,
        ack_bits,
    };

    let mut buffer = vec![0u8; header.size()];
    header.write(&mut buffer)?;
    buffer.extend_from_slice(&payload);

    Ok(buffer)
}

pub fn build_fragments(
    payload: &[u8],
    sequence: u16,
    ack: u16,
    ack_bits: u32,
    config: &Config,
) -> Result<Vec<Vec<u8>>> {
    let packet_bytes = payload.len();
    let exact_division = ((packet_bytes % config.fragment_size) != 0) as usize;
    let num_fragments = packet_bytes / config.fragment_size + exact_division;

    if num_fragments > config.max_fragments {
        return Err(RenetError::MaximumFragmentsExceeded);
    }

    let mut fragments = Vec::with_capacity(num_fragments);
    for id in 0..num_fragments {
        let start = config.fragment_size * id;
        let mut end = config.fragment_size * (id + 1);
        if packet_bytes < end {
            end = packet_bytes;
        }
        let mut packet_header = None;
        if id == 0 {
            packet_header = Some(PacketHeader {
                sequence,
                ack,
                ack_bits,
            });
        }
        let fragment_header = FragmentHeader {
            fragment_id: id as u8,
            sequence,
            num_fragments: num_fragments as u8,
            packet_header,
        };
        let mut buffer = vec![0u8; fragment_header.size()];
        fragment_header.write(&mut buffer).unwrap();
        buffer.extend_from_slice(&payload[start..end]);
        fragments.push(buffer);
    }

    Ok(fragments)
}

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(
        &mut self,
        header: FragmentHeader,
        payload: &[u8],
        config: &Config,
    ) -> Result<Option<Vec<u8>>> {
        if !self.exists(header.sequence) {
            let reassembly_fragment =
                ReassemblyFragment::new(header.sequence, header.num_fragments as usize);
            self.insert(header.sequence, reassembly_fragment);
        }

        let reassembly_fragment = match self.get_mut(header.sequence) {
            Some(x) => x,
            None => return Err(RenetError::CouldNotFindFragment),
        };

        if reassembly_fragment.num_fragments_total != header.num_fragments as usize {
            return Err(RenetError::InvalidNumberFragment);
        }

        if header.fragment_id as usize >= reassembly_fragment.num_fragments_total {
            return Err(RenetError::MaximumFragmentsExceeded);
        }

        if reassembly_fragment.fragments_received[header.fragment_id as usize] {
            return Err(RenetError::FragmentAlreadyProcessed);
        }

        reassembly_fragment.num_fragments_received += 1;
        reassembly_fragment.fragments_received[header.fragment_id as usize] = true;

        // Resize buffer to fit the last fragment size
        if header.fragment_id == header.num_fragments - 1 {
            let len = (reassembly_fragment.num_fragments_total - 1) * config.fragment_size
                + payload.len();
            reassembly_fragment.buffer.resize(len, 0);
        }

        let mut cursor = Cursor::new(reassembly_fragment.buffer.as_mut_slice());
        cursor.set_position(header.fragment_id as u64 * config.fragment_size as u64);
        cursor.write_all(payload)?;

        if reassembly_fragment.num_fragments_received == reassembly_fragment.num_fragments_total {
            drop(reassembly_fragment);
            if let Some(reassembly_fragment) = self.remove(header.sequence) {
                return Ok(Some(reassembly_fragment.buffer.clone()));
            } else {
                return Err(RenetError::CouldNotFindFragment);
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment() {
        let payload = [0u8; 2500];
        let config = Config::default();
            let fragments = build_fragments(&payload, 0, 0, 0, &config).unwrap();
        let mut fragments_reassembly: SequenceBuffer<ReassemblyFragment> =
            SequenceBuffer::with_capacity(256);
        assert_eq!(3, fragments.len());

        let mut fragments: Vec<&[u8]> = fragments.iter().map(|x| &x[..]).collect();

        let header = FragmentHeader::parse(&fragments[0]).unwrap();
        fragments[0] = &fragments[0][header.size()..];
        assert!(fragments_reassembly
            .handle_fragment(header, &fragments[0], &config)
            .unwrap()
            .is_none());
        let header = FragmentHeader::parse(&fragments[1]).unwrap();
        fragments[1] = &fragments[1][header.size()..];
        assert!(fragments_reassembly
            .handle_fragment(header, &fragments[1], &config)
            .unwrap()
            .is_none());
        let header = FragmentHeader::parse(&fragments[2]).unwrap();
        fragments[2] = &fragments[2][header.size()..];
        let reassembly_payload = fragments_reassembly
            .handle_fragment(header, &fragments[2], &config)
            .unwrap()
            .unwrap();

        assert_eq!(reassembly_payload.len(), payload.len());
        assert!(reassembly_payload
            .iter()
            .zip(payload.iter())
            .all(|(a, b)| a == b));
    }
}
