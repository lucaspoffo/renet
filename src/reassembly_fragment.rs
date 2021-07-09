use crate::packet::{AckData, Fragment, Message};
use crate::sequence_buffer::SequenceBuffer;
use crate::RenetError;

use log::{error, trace};
use std::io;
use std::io::{Cursor, Write};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct FragmentConfig {
    pub max_fragments: usize,
    pub fragment_above: usize,
    pub fragment_size: usize,
    pub max_count: usize,
    pub reassembly_buffer_size: usize,
}

#[derive(Clone)]
pub struct ReassemblyFragment {
    sequence: u16,
    num_fragments_received: u32,
    num_fragments_total: u32,
    buffer: Vec<u8>,
    fragments_received: Vec<bool>,
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
    pub fn new(sequence: u16, num_fragments_total: u32, fragment_size: usize) -> Self {
        let len = (num_fragments_total as usize * fragment_size) as usize;
        let buffer = vec![0; len];

        Self {
            sequence,
            num_fragments_received: 0,
            num_fragments_total,
            buffer,
            fragments_received: vec![false; num_fragments_total as usize],
        }
    }
}

#[derive(Debug, Error)]
pub enum FragmentError {
    #[error("fragment with sequence {sequence} has invalid number of fragments, expected {expected}, got {got}.")]
    InvalidTotalFragment {
        sequence: u16,
        expected: u32,
        got: u8,
    },
    #[error("fragment with sequence {sequence} has invalid id {id}, expected < {total}. ")]
    InvalidFragmentId { sequence: u16, id: u8, total: u32 },
    #[error("fragment with sequence {sequence} and id {id} fragment already processed.")]
    AlreadyProcessed { sequence: u16, id: u8 },
    #[error("writing fragment failed: {0}")]
    IOError(io::Error),
    #[error("fragmentation with sequence {sequence} exceeded maximum count, got {got}, expected < {expected}")]
    ExceededMaxFragmentCount {
        sequence: u16,
        expected: usize,
        got: usize,
    },
}

impl From<io::Error> for FragmentError {
    fn from(inner: io::Error) -> FragmentError {
        FragmentError::IOError(inner)
    }
}

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(
        &mut self,
        fragment: Fragment,
        config: &FragmentConfig,
    ) -> Result<Option<Vec<u8>>, FragmentError> {
        let Fragment {
            sequence,
            num_fragments,
            payload,
            fragment_id,
            ..
        } = fragment;

        if !self.exists(sequence) {
            let reassembly_fragment =
                ReassemblyFragment::new(sequence, num_fragments as u32, config.fragment_size);
            self.insert(sequence, reassembly_fragment);
        }

        // TODO: Maybe add get_or_insert
        let reassembly_fragment = self.get_mut(sequence).unwrap();

        if reassembly_fragment.num_fragments_total != num_fragments as u32 {
            return Err(FragmentError::InvalidTotalFragment {
                sequence,
                expected: reassembly_fragment.num_fragments_total,
                got: num_fragments,
            });
        }

        if fragment_id as u32 >= reassembly_fragment.num_fragments_total {
            return Err(FragmentError::InvalidFragmentId {
                sequence,
                id: fragment_id,
                total: reassembly_fragment.num_fragments_total,
            });
        }

        if reassembly_fragment.fragments_received[fragment_id as usize] {
            return Err(FragmentError::AlreadyProcessed {
                sequence,
                id: fragment_id,
            });
        }

        reassembly_fragment.num_fragments_received += 1;
        reassembly_fragment.fragments_received[fragment_id as usize] = true;

        trace!(
            "Received fragment {} of packet {} ({}/{})",
            fragment_id,
            sequence,
            reassembly_fragment.num_fragments_received,
            reassembly_fragment.num_fragments_total
        );

        // Resize buffer to fit the last fragment size
        if fragment_id == num_fragments - 1 {
            let len = (reassembly_fragment.num_fragments_total - 1) as usize * config.fragment_size
                + payload.len();
            reassembly_fragment.buffer.resize(len, 0);
        }

        let mut cursor = Cursor::new(reassembly_fragment.buffer.as_mut_slice());
        cursor.set_position(fragment_id as u64 * config.fragment_size as u64);
        cursor.write_all(&payload)?;

        if reassembly_fragment.num_fragments_received == reassembly_fragment.num_fragments_total {
            let reassembly_fragment = self.remove(sequence).unwrap();
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
    ack_data: AckData,
    config: &FragmentConfig,
) -> Result<Vec<Vec<u8>>, RenetError> {
    let packet_bytes = payload.len();
    let exact_division = ((packet_bytes % config.fragment_size) != 0) as usize;
    let num_fragments = packet_bytes / config.fragment_size + exact_division;

    if num_fragments > config.max_fragments {
        error!(
            "Fragmentation exceeded maximum number of fragments, got {}, maximum is {}.",
            num_fragments, config.max_fragments
        );
        return Err(RenetError::FragmentError(
            FragmentError::ExceededMaxFragmentCount {
                sequence,
                expected: config.max_fragments,
                got: num_fragments,
            },
        ));
    }

    let mut fragments = Vec::with_capacity(num_fragments);
    for (id, chunk) in payload.chunks(config.fragment_size).enumerate() {
        let fragment: Message = Fragment {
            fragment_id: id as u8,
            sequence,
            num_fragments: num_fragments as u8,
            ack_data,
            payload: chunk.into(),
        }
        .into();
        let fragment = bincode::serialize(&fragment)?;
        fragments.push(fragment);
    }

    Ok(fragments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment() {
        let payload = vec![1u8; 2500];
        let config = FragmentConfig::default();
        let ack_data = AckData {
            ack: 0,
            ack_bits: 0,
        };
        let fragments = build_fragments(&payload, 0, ack_data, &config).unwrap();
        let mut fragments_reassembly: SequenceBuffer<ReassemblyFragment> =
            SequenceBuffer::with_capacity(256);
        assert_eq!(3, fragments.len());

        let fragments: Vec<Fragment> = fragments
            .iter()
            .map(|payload| {
                let fragment: Message = bincode::deserialize(payload).unwrap();
                match fragment {
                    Message::Fragment(f) => f,
                    _ => panic!(),
                }
            })
            .collect();

        let result = fragments_reassembly.handle_fragment(fragments[0].clone(), &config);
        assert!(matches!(result, Ok(None)));

        let result = fragments_reassembly.handle_fragment(fragments[1].clone(), &config);
        assert!(matches!(result, Ok(None)));

        let result = fragments_reassembly.handle_fragment(fragments[2].clone(), &config);
        let result = result.unwrap().unwrap();

        assert_eq!(result, payload);
    }
}
