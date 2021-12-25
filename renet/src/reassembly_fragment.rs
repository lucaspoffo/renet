use crate::packet::{AckData, ChannelMessages, Fragment, Packet, Payload};
use crate::sequence_buffer::SequenceBuffer;

use bincode::Options;
use log::{error, trace};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct FragmentConfig {
    pub fragment_above: u64,
    pub fragment_size: usize,
    pub reassembly_buffer_size: usize,
}

#[derive(Debug, Clone)]
pub struct ReassemblyFragment {
    sequence: u16,
    num_fragments_received: u8,
    num_fragments_total: u8,
    buffer: Vec<u8>,
    fragments_received: Vec<bool>,
}

impl Default for FragmentConfig {
    fn default() -> Self {
        Self {
            fragment_above: 1024,
            fragment_size: 1024,
            reassembly_buffer_size: 256,
        }
    }
}

impl ReassemblyFragment {
    pub fn new(sequence: u16, num_fragments_total: u8, fragment_size: usize) -> Self {
        let len = num_fragments_total as usize * fragment_size;
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
        expected: u8,
        got: u8,
    },
    #[error("fragment with sequence {sequence} has invalid id {id}, expected < {total}. ")]
    InvalidFragmentId { sequence: u16, id: u8, total: u8 },
    #[error("fragment with sequence {sequence} and id {id} fragment already processed.")]
    AlreadyProcessed { sequence: u16, id: u8 },
    #[error("fragmentation with sequence {sequence} exceeded maximum count, got {got}, expected < {expected}")]
    ExceededMaxFragmentCount {
        sequence: u16,
        expected: u8,
        got: u8,
    },
    #[error("fragment with sequence {sequence} is too old")]
    OldSequence { sequence: u16 },
    #[error("bincode failed to (de)serialize: {0}")]
    BincodeError(#[from] bincode::Error),
}

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(
        &mut self,
        fragment: Fragment,
        max_packet_size: u64,
        config: &FragmentConfig,
    ) -> Result<Option<ChannelMessages>, FragmentError> {
        let Fragment {
            sequence,
            num_fragments,
            payload,
            fragment_id,
            ..
        } = fragment;

        let reassembly_fragment = self
            .get_or_insert_with(sequence, || {
                ReassemblyFragment::new(sequence, num_fragments, config.fragment_size)
            })
            .ok_or(FragmentError::OldSequence { sequence })?;

        let max_fragments = (max_packet_size / config.fragment_size as u64) + 1;
        assert!(
            max_fragments <= 255,
            "Limit for fragments allowed is 255, got {}, reduce the max packet size or increase the fragment size", max_fragments
        );

        if reassembly_fragment.num_fragments_total as u64 > max_fragments {
            return Err(FragmentError::ExceededMaxFragmentCount {
                sequence,
                expected: max_fragments as u8,
                got: num_fragments,
            });
        }

        if reassembly_fragment.num_fragments_total != num_fragments {
            return Err(FragmentError::InvalidTotalFragment {
                sequence,
                expected: reassembly_fragment.num_fragments_total,
                got: num_fragments,
            });
        }

        if fragment_id >= reassembly_fragment.num_fragments_total {
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
            let len = (reassembly_fragment.num_fragments_total - 1) as usize
                * config.fragment_size as usize
                + payload.len();
            reassembly_fragment.buffer.resize(len, 0);
        }

        let start = fragment_id as usize * config.fragment_size as usize;
        reassembly_fragment.buffer[start..start + payload.len()].copy_from_slice(&payload);
        if reassembly_fragment.num_fragments_received == reassembly_fragment.num_fragments_total {
            let reassembly_fragment = self
                .remove(sequence)
                .expect("ReassemblyFragment always exists here");

            let messages: ChannelMessages =
                bincode::options().deserialize(&reassembly_fragment.buffer)?;

            trace!(
                "Completed the reassembly of packet {}.",
                reassembly_fragment.sequence
            );
            return Ok(Some(messages));
        }

        Ok(None)
    }
}

pub(crate) fn build_fragments(
    messages: ChannelMessages,
    sequence: u16,
    ack_data: AckData,
    config: &FragmentConfig,
) -> Result<Vec<Payload>, bincode::Error> {
    let payload = bincode::options().serialize(&messages)?;
    let packet_bytes = payload.len();
    let exact_division = (packet_bytes % config.fragment_size != 0) as usize;
    let num_fragments = packet_bytes / config.fragment_size + exact_division;

    let mut fragments = Vec::with_capacity(num_fragments);
    for (id, chunk) in payload.chunks(config.fragment_size).enumerate() {
        let fragment = Packet::Fragment(Fragment {
            fragment_id: id as u8,
            sequence,
            num_fragments: num_fragments as u8,
            ack_data,
            payload: chunk.into(),
        });
        let fragment = bincode::options().serialize(&fragment)?;
        fragments.push(fragment);
    }

    Ok(fragments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment() {
        let config = FragmentConfig::default();
        let ack_data = AckData {
            ack: 0,
            ack_bits: 0,
        };

        let messages = ChannelMessages {
            slice_messages: vec![],
            unreliable_messages: vec![vec![7u8; 1000], vec![255u8; 1000], vec![33u8; 1000]],
            reliable_channels_data: vec![],
        };

        let fragments = build_fragments(messages.clone(), 0, ack_data, &config).unwrap();
        let mut fragments_reassembly: SequenceBuffer<ReassemblyFragment> =
            SequenceBuffer::with_capacity(256);
        assert_eq!(3, fragments.len());

        let fragments: Vec<Fragment> = fragments
            .iter()
            .map(|payload| {
                let fragment: Packet = bincode::options().deserialize(payload).unwrap();
                match fragment {
                    Packet::Fragment(f) => f,
                    _ => panic!(),
                }
            })
            .collect();

        let result = fragments_reassembly.handle_fragment(fragments[0].clone(), 250_000, &config);
        assert!(matches!(result, Ok(None)));

        let result = fragments_reassembly.handle_fragment(fragments[1].clone(), 250_000, &config);
        assert!(matches!(result, Ok(None)));

        let result = fragments_reassembly.handle_fragment(fragments[2].clone(), 250_000, &config);
        let result = result.unwrap().unwrap();

        assert_eq!(
            messages.unreliable_messages.len(),
            result.unreliable_messages.len()
        );
        assert_eq!(
            messages.unreliable_messages[0],
            result.unreliable_messages[0]
        );
        assert_eq!(
            messages.unreliable_messages[1],
            result.unreliable_messages[1]
        );
        assert_eq!(
            messages.unreliable_messages[2],
            result.unreliable_messages[2]
        );
    }
}
