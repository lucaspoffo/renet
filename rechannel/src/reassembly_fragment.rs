use crate::packet::{AckData, ChannelPacketData, FragmentData, Packet, Payload};
use crate::sequence_buffer::SequenceBuffer;

use bincode::Options;

use std::error::Error;
use std::fmt;

/// Configuration for how the packet fragmentation will occur.
#[derive(Debug, Clone)]
pub struct FragmentConfig {
    /// Packets are fragmented when size (bytes) is above.
    pub fragment_above: u64,
    /// Packet is split up into fragments of this size (bytes).
    pub fragment_size: usize,
    /// Number of packet entries in the fragmentation reassembly sequence buffer.
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

#[derive(Debug)]
pub enum FragmentError {
    /// Fragment has invalid total fragment number
    InvalidTotalFragment { sequence: u16, expected: u8, got: u8 },
    /// Fragment contained an invalid id.
    InvalidFragmentId { sequence: u16, id: u8, total: u8 },
    /// Tried to process duplicated fragment
    AlreadyProcessed { sequence: u16, id: u8 },
    /// Fragment contained fragment count above the limit set by the configuration
    ExceededMaxFragmentCount { sequence: u16, expected: u8, got: u8 },
    /// Fragment too old to be processed
    OldSequence { sequence: u16 },
    /// (De)serialization error
    BincodeError(bincode::Error),
}

impl fmt::Display for FragmentError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use FragmentError::*;

        match *self {
            InvalidTotalFragment { sequence, expected, got } => {
                write!(
                    fmt,
                    "fragment with sequence {} has invalid number of fragments, expected {}, got {}",
                    sequence, expected, got
                )
            }
            InvalidFragmentId { sequence, id, total } => {
                write!(
                    fmt,
                    "fragment with sequence {} has invalid id {}, expected < {}",
                    sequence, id, total
                )
            }
            AlreadyProcessed { sequence, id } => {
                write!(fmt, "fragment with sequence {} and id {} fragment already processed.", sequence, id)
            }
            ExceededMaxFragmentCount { sequence, expected, got } => {
                write!(
                    fmt,
                    "fragmentation with sequence {} exceeded maximum count, got {}, expected < {}",
                    sequence, got, expected
                )
            }
            OldSequence { sequence } => write!(fmt, "fragment with sequence {} is too old", sequence),
            BincodeError(ref bincode_err) => write!(fmt, "bincode error: {}", bincode_err),
        }
    }
}

impl Error for FragmentError {}

impl From<bincode::Error> for FragmentError {
    fn from(inner: bincode::Error) -> Self {
        FragmentError::BincodeError(inner)
    }
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

impl FragmentConfig {
    /// Asserts that the configuration can fragment packets with the given size.
    pub(crate) fn assert_can_fragment_packet_with_size(&self, packet_size: u64) {
        let max_fragments = self.num_fragments(packet_size);
        assert!(
            max_fragments <= 256,
            "Invalid fragmentation configuration: limit for fragments allowed is 256, got {}. \
            Reduce the max packet size or increase the fragment size",
            max_fragments
        );
    }

    fn num_fragments(&self, packet_size: u64) -> u64 {
        let not_exact_division = u64::from(packet_size % self.fragment_size as u64 != 0);
        (packet_size / self.fragment_size as u64) + not_exact_division
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

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(
        &mut self,
        sequence: u16,
        fragment_data: FragmentData,
        max_packet_size: u64,
        config: &FragmentConfig,
    ) -> Result<Option<Vec<ChannelPacketData>>, FragmentError> {
        let FragmentData {
            fragment_id,
            num_fragments,
            payload,
        } = fragment_data;
        let reassembly_fragment = self
            .get_or_insert_with(sequence, || ReassemblyFragment::new(sequence, num_fragments, config.fragment_size))
            .ok_or(FragmentError::OldSequence { sequence })?;

        let max_fragments = config.num_fragments(max_packet_size);

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
            return Err(FragmentError::AlreadyProcessed { sequence, id: fragment_id });
        }

        reassembly_fragment.num_fragments_received += 1;
        reassembly_fragment.fragments_received[fragment_id as usize] = true;

        log::trace!(
            "Received fragment {} of packet {} ({}/{})",
            fragment_id,
            sequence,
            reassembly_fragment.num_fragments_received,
            reassembly_fragment.num_fragments_total
        );

        // Resize buffer to fit the last fragment size
        if fragment_id == num_fragments - 1 {
            let len = (reassembly_fragment.num_fragments_total - 1) as usize * config.fragment_size + payload.len();
            reassembly_fragment.buffer.resize(len, 0);
        }

        let start = fragment_id as usize * config.fragment_size;
        reassembly_fragment.buffer[start..start + payload.len()].copy_from_slice(&payload);
        if reassembly_fragment.num_fragments_received == reassembly_fragment.num_fragments_total {
            let reassembly_fragment = self.remove(sequence).expect("ReassemblyFragment always exists here");

            let messages: Vec<ChannelPacketData> = bincode::options().deserialize(&reassembly_fragment.buffer)?;

            log::trace!("Completed the reassembly of packet {}.", reassembly_fragment.sequence);
            return Ok(Some(messages));
        }

        Ok(None)
    }
}

pub(crate) fn build_fragments(
    channels_packet_data: Vec<ChannelPacketData>,
    sequence: u16,
    ack_data: AckData,
    config: &FragmentConfig,
) -> Result<Vec<Payload>, bincode::Error> {
    let payload = bincode::options().serialize(&channels_packet_data)?;
    let packet_bytes = payload.len();
    let exact_division = (packet_bytes % config.fragment_size != 0) as usize;
    let num_fragments = packet_bytes / config.fragment_size + exact_division;

    let mut fragments = Vec::with_capacity(num_fragments);
    for (id, chunk) in payload.chunks(config.fragment_size).enumerate() {
        let fragment = Packet::Fragment {
            sequence,
            ack_data,
            fragment_data: FragmentData {
                fragment_id: id as u8,
                num_fragments: num_fragments as u8,
                payload: chunk.into(),
            },
        };
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
        let ack_data = AckData { ack: 0, ack_bits: 0 };

        let messages = vec![ChannelPacketData {
            channel_id: 0,
            messages: vec![vec![7u8; 1000], vec![255u8; 1000], vec![33u8; 1000]],
        }];
        let sequence = 0;

        let fragments = build_fragments(messages.clone(), sequence, ack_data, &config).unwrap();
        let mut fragments_reassembly: SequenceBuffer<ReassemblyFragment> = SequenceBuffer::with_capacity(256);
        assert_eq!(3, fragments.len());

        let fragments: Vec<FragmentData> = fragments
            .iter()
            .map(|payload| {
                let fragment: Packet = bincode::options().deserialize(payload).unwrap();
                match fragment {
                    Packet::Fragment { fragment_data, .. } => fragment_data,
                    _ => panic!(),
                }
            })
            .collect();

        let result = fragments_reassembly.handle_fragment(sequence, fragments[0].clone(), 250_000, &config);
        match result {
            Ok(payloads) => assert!(payloads.is_none()),
            _ => unreachable!(),
        }

        let result = fragments_reassembly.handle_fragment(sequence, fragments[1].clone(), 250_000, &config);
        match result {
            Ok(payloads) => assert!(payloads.is_none()),
            _ => unreachable!(),
        }

        let result = fragments_reassembly.handle_fragment(sequence, fragments[2].clone(), 250_000, &config);
        let result = result.unwrap().unwrap();

        assert_eq!(messages.len(), result.len());

        assert_eq!(messages[0], result[0]);
    }
}
