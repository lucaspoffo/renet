use self::sequence_buffer::SequenceBuffer;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Write};
use error::{Result, RenetError};
use self::packet::{FragmentHeader, FRAGMENT_MAX_COUNT, FRAGMENT_MAX_SIZE};

mod sequence_buffer;
mod error;
mod packet;

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

pub fn build_fragments<'a>(payload: &'a [u8], config: &Config) -> Result<Vec<&'a [u8]>> {
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
        let data = &payload[start..end];
        fragments.push(data);
    }

    Ok(fragments)
}

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(
        &mut self,
        header: FragmentHeader,
        payload: &[u8],
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
        let fragments = build_fragments(&payload, &config).unwrap();
        let mut fragments_reassembly: SequenceBuffer<ReassemblyFragment> =
            SequenceBuffer::with_capacity(256);
        let mut headers = vec![];
        for i in 0..fragments.len() {
            let header = FragmentHeader {
                sequence: 0,
                fragment_id: i as u8,
                num_fragments: fragments.len() as u8,
            };
            headers.push(header);
        }
        assert_eq!(4, fragments.len());
        assert!(fragments_reassembly
            .handle_fragment(headers[0].clone(), fragments[0])
            .unwrap()
            .is_none());
        assert!(fragments_reassembly
            .handle_fragment(headers[1].clone(), fragments[1])
            .unwrap()
            .is_none());
        assert!(fragments_reassembly
            .handle_fragment(headers[2].clone(), fragments[2])
            .unwrap()
            .is_none());
        let reassembly_payload = fragments_reassembly
            .handle_fragment(headers[3].clone(), fragments[3])
            .unwrap()
            .unwrap();

        assert_eq!(reassembly_payload.len(), payload.len());
        assert!(reassembly_payload
            .iter()
            .zip(payload.iter())
            .all(|(a, b)| a == b));
    }
}
