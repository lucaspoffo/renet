use self::packet::{
    FragmentHeader, HeaderParser, PacketHeader, FRAGMENT_MAX_COUNT, FRAGMENT_MAX_SIZE, PacketType
};
use self::sequence_buffer::SequenceBuffer;
use async_std::net::UdpSocket;
use error::{RenetError, Result};
use futures::future::try_join_all;
use std::io::{Cursor, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use log::trace;

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
        Self {
            sequence,
            num_fragments_received: 0,
            num_fragments_total,
            buffer: Vec::with_capacity(num_fragments_total * FRAGMENT_MAX_SIZE),
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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_packet_size: 16 * 1024,
            max_fragments: 32,
            fragment_above: 1024,
            fragment_size: 1024,
            fragment_reassembly_buffer_size: 256,
        }
    }
}

pub struct Endpoint {
    time: f64,
    rtt: f32,
    config: Config,
    sequence: AtomicU16,
    reassembly_buffer: SequenceBuffer<ReassemblyFragment>,
    socket: UdpSocket,
}

impl Endpoint {
    pub fn new(config: Config, time: f64, socket: UdpSocket) -> Self {
        Self {
            time,
            rtt: 0.0,
            sequence: AtomicU16::new(0),
            reassembly_buffer: SequenceBuffer::with_capacity(
                config.fragment_reassembly_buffer_size,
            ),
            config,
            socket,
        }
    }

    pub async fn send_to(&self, payload: &[u8], addrs: SocketAddr) -> Result<()> {
        if payload.len() > self.config.max_packet_size {
            return Err(RenetError::MaximumPacketSizeExceeded);
        }

        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        if payload.len() > self.config.fragment_above {
            // Fragment packet
            let fragments = build_fragments(payload, sequence, &self.config)?;
            let futures = fragments.iter().map(|f| self.socket.send_to(&f, addrs));
            try_join_all(futures).await?;
        } else {
            // Normal packet
            let packet = build_normal_packet(payload, sequence)?;
            self.socket.send_to(&packet, addrs).await?;
        }

        Ok(())
    }

    pub async fn receive(&mut self, buf: &mut [u8]) -> Result<Option<Vec<u8>>> {
        let (n, addrs) = self.socket.recv_from(buf).await?;
        trace!("Received {} bytes from {}.", n, addrs);
        let payload = &buf[..n];
        if payload[0] == PacketType::Packet as u8 {
            let header_payload = &payload[..PacketHeader::size()];
            let mut cursor = Cursor::new(header_payload);
            let header = PacketHeader::parse(&mut cursor)?;
            trace!("Received: {:?}", header);

            let payload = &payload[FragmentHeader::size()..];
            return Ok(Some(payload.into()));
        } else {
            let payload = self.reassembly_buffer.handle_fragment(payload)?;
            return Ok(payload);
        }
    }
}

pub fn build_normal_packet(payload: &[u8], sequence: u16) -> Result<Vec<u8>> {
    let header = PacketHeader { sequence };

    let mut buffer = vec![0u8; PacketHeader::size()];
    let mut cursor = Cursor::new(buffer.as_mut_slice());
    header.write(&mut cursor)?;
    buffer.extend_from_slice(&payload);

    Ok(buffer)
}

pub fn build_fragments(payload: &[u8], sequence: u16, config: &Config) -> Result<Vec<Vec<u8>>> {
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
        let fragment_header = FragmentHeader {
            fragment_id: id as u8,
            sequence,
            num_fragments: num_fragments as u8,
        };
        let mut buffer = vec![0u8; FragmentHeader::size()];
        let mut cursor = Cursor::new(buffer.as_mut_slice());
        fragment_header.write(&mut cursor).unwrap();
        buffer.extend_from_slice(&payload[start..end]);
        fragments.push(buffer);
    }

    Ok(fragments)
}

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(&mut self, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let header_payload = &payload[..FragmentHeader::size()];
        let mut cursor = Cursor::new(header_payload);
        let header = FragmentHeader::parse(&mut cursor)?;

        let payload = &payload[FragmentHeader::size()..];
        trace!("Received: {:?}", header);
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
        reassembly_fragment.buffer.write_all(&*payload)?;

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
        let payload = [7u8; 3500];
        let config = Config::default();
        let fragments = build_fragments(&payload, 0, &config).unwrap();
        let mut fragments_reassembly: SequenceBuffer<ReassemblyFragment> =
            SequenceBuffer::with_capacity(256);
        assert_eq!(4, fragments.len());
        assert!(fragments_reassembly
            .handle_fragment(&fragments[0])
            .unwrap()
            .is_none());
        assert!(fragments_reassembly
            .handle_fragment(&fragments[1])
            .unwrap()
            .is_none());
        assert!(fragments_reassembly
            .handle_fragment(&fragments[2])
            .unwrap()
            .is_none());
        let reassembly_payload = fragments_reassembly
            .handle_fragment(&fragments[3])
            .unwrap()
            .unwrap();

        assert_eq!(reassembly_payload.len(), payload.len());
        assert!(reassembly_payload
            .iter()
            .zip(payload.iter())
            .all(|(a, b)| a == b));
    }
}
