use bytes::Bytes;

use crate::{
    error::ChannelError,
    packet::SLICE_SIZE
};

#[derive(Debug, Clone)]
pub struct SliceConstructor {
    message_id: u64,
    pub num_slices: usize,
    num_received_slices: usize,
    received: Vec<bool>,
    sliced_data: Vec<u8>,
}

impl SliceConstructor {
    pub fn new(message_id: u64, num_slices: usize) -> Self {
        SliceConstructor {
            message_id,
            num_slices,
            num_received_slices: 0,
            received: vec![false; num_slices],
            sliced_data: vec![0; num_slices * SLICE_SIZE],
        }
    }

    pub fn process_slice(&mut self, slice_index: usize, bytes: &[u8]) -> Result<Option<Bytes>, ChannelError> {
        let is_last_slice = slice_index == self.num_slices - 1;
        if is_last_slice {
            if bytes.len() > SLICE_SIZE {
                log::error!(
                    "Invalid last slice_size for SliceMessage, got {}, expected less than {}.",
                    bytes.len(),
                    SLICE_SIZE,
                );
                return Err(ChannelError::InvalidSliceMessage);
            }
        } else if bytes.len() != SLICE_SIZE {
            log::error!("Invalid slice_size for SliceMessage, got {}, expected {}.", bytes.len(), SLICE_SIZE);
            return Err(ChannelError::InvalidSliceMessage);
        }

        if !self.received[slice_index] {
            self.received[slice_index] = true;
            self.num_received_slices += 1;

            if is_last_slice {
                let len = (self.num_slices - 1) * SLICE_SIZE + bytes.len();
                self.sliced_data.resize(len, 0);
            }

            let start = slice_index * SLICE_SIZE;
            let end = if slice_index == self.num_slices - 1 {
                (self.num_slices - 1) * SLICE_SIZE + bytes.len()
            } else {
                (slice_index + 1) * SLICE_SIZE
            };

            self.sliced_data[start..end].copy_from_slice(bytes);
            log::trace!(
                "Received slice {} from message {}. ({}/{})",
                slice_index,
                self.message_id,
                self.num_received_slices,
                self.num_slices
            );
        }

        if self.num_received_slices == self.num_slices {
            log::trace!("Received all slices for message {}.", self.message_id);
            let payload = std::mem::take(&mut self.sliced_data);
            return Ok(Some(payload.into()));
        }

        Ok(None)
    }
}
