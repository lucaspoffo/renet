use crate::packet::{FragmentHeader, HeaderParser, PacketHeader};
use crate::sequence_buffer::SequenceBuffer;

use log::{error, trace};

use std::io;
use std::io::{Cursor, Write};

#[derive(Clone)]
pub struct ReassemblyFragment {
    sequence: u16,
    num_fragments_received: usize,
    num_fragments_total: usize,
    buffer: Vec<u8>,
    fragments_received: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct FragmentConfig {
    pub max_fragments: usize,
    pub fragment_above: usize,
    pub fragment_size: usize,
    pub max_count: usize,
    pub reassembly_buffer_size: usize,
}

impl Default for FragmentConfig {
    fn default() -> Self {
        Self {
            max_fragments: 32,
            fragment_above: 1024,
            fragment_size: 1024,
            max_count: 256,
            reassembly_buffer_size: 256,
        }
    }
}

impl ReassemblyFragment {
    pub fn new(sequence: u16, num_fragments_total: usize, fragment_size: usize) -> Self {
        let len = num_fragments_total * fragment_size;
        let buffer = vec![0; len];

        Self {
            sequence,
            num_fragments_received: 0,
            num_fragments_total,
            buffer,
            fragments_received: vec![false; num_fragments_total],
        }
    }
}

#[derive(Debug)]
pub enum FragmentError {
    /// Current header fragment count does not match others fragments.
    InvalidTotalFragment,
    InvalidFragmentId,
    ReassemblerNotFound,
    AlreadyProcessed,
    IOError(io::Error),
    ExceededMaxFragmentCount,
}

impl From<io::Error> for FragmentError {
    fn from(inner: io::Error) -> FragmentError {
        FragmentError::IOError(inner)
    }
}

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(
        &mut self,
        header: FragmentHeader,
        payload: &[u8],
        config: &FragmentConfig,
    ) -> Result<Option<Vec<u8>>, FragmentError> {
        if !self.exists(header.sequence) {
            let reassembly_fragment = ReassemblyFragment::new(
                header.sequence,
                header.num_fragments as usize,
                config.fragment_size,
            );
            self.insert(header.sequence, reassembly_fragment);
        }

        let reassembly_fragment = match self.get_mut(header.sequence) {
            Some(x) => x,
            None => {
                error!("Could not find fragment reassembler {}", header.sequence);
                return Err(FragmentError::ReassemblerNotFound);
            }
        };

        if reassembly_fragment.num_fragments_total != header.num_fragments as usize {
            error!(
                "Ignoring packet with invalid number of fragments, expected {}, got {}.",
                reassembly_fragment.num_fragments_total, header.num_fragments
            );
            return Err(FragmentError::InvalidTotalFragment);
        }

        if header.fragment_id as usize >= reassembly_fragment.num_fragments_total {
            error!(
                "Ignoring fragment {} of packet {} with invalid fragment id",
                header.fragment_id, header.sequence
            );
            return Err(FragmentError::InvalidFragmentId);
        }

        if reassembly_fragment.fragments_received[header.fragment_id as usize] {
            error!(
                "Ignoring fragment {} of packet {}, fragment already processed.",
                header.fragment_id, header.sequence
            );
            return Err(FragmentError::AlreadyProcessed);
        }

        reassembly_fragment.num_fragments_received += 1;
        reassembly_fragment.fragments_received[header.fragment_id as usize] = true;

        trace!(
            "Received fragment {} of packet {} ({}/{})",
            header.fragment_id,
            header.sequence,
            reassembly_fragment.num_fragments_received,
            reassembly_fragment.num_fragments_total
        );

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
            let reassembly_fragment = self.remove(header.sequence).unwrap();
            trace!(
                "Completed the reassembly of packet {}.",
                reassembly_fragment.sequence
            );
            return Ok(Some(reassembly_fragment.buffer));
        }

        Ok(None)
    }
}

pub fn build_fragments(
    payload: &[u8],
    sequence: u16,
    ack: u16,
    ack_bits: u32,
    config: &FragmentConfig,
) -> Result<Vec<Vec<u8>>, FragmentError> {
    let packet_bytes = payload.len();
    let exact_division = ((packet_bytes % config.fragment_size) != 0) as usize;
    let num_fragments = packet_bytes / config.fragment_size + exact_division;

    if num_fragments > config.max_fragments {
        error!(
            "Fragmentation exceeded maximum number of fragments, got {}, maximum is {}.",
            num_fragments, config.max_fragments
        );
        return Err(FragmentError::ExceededMaxFragmentCount);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment() {
        let payload = [1u8; 2500];
        let config = FragmentConfig::default();
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
