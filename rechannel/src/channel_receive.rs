use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    another_channel::{Slice, SLICE_SIZE},
    error::ChannelError,
};

#[derive(Debug, Clone)]
struct SliceConstructor {
    message_id: u64,
    num_slices: usize,
    num_received_slices: usize,
    received: Vec<bool>,
    sliced_data: Vec<u8>,
}

enum ReliableOrder {
    Ordered,
    Unordered {
        most_recent_message_id: u64,
        received_messages: BTreeSet<u64>,
    },
}

impl SliceConstructor {
    fn new(message_id: u64, num_slices: usize) -> Self {
        SliceConstructor {
            message_id,
            num_slices,
            num_received_slices: 0,
            received: vec![false; num_slices as usize],
            sliced_data: vec![0; num_slices as usize * SLICE_SIZE],
        }
    }

    fn process_slice(&mut self, slice_index: usize, bytes: &[u8]) -> Result<Option<Bytes>, ChannelError> {
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

            self.sliced_data[start..end].copy_from_slice(&bytes);
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

struct ReceiveChannelUnreliable {
    messages: VecDeque<Bytes>,
    slices: HashMap<u64, SliceConstructor>,
    slices_last_received: HashMap<u64, Duration>,
    error: Option<ChannelError>,
}

impl ReceiveChannelUnreliable {
    fn new() -> Self {
        Self {
            slices: HashMap::new(),
            slices_last_received: HashMap::new(),
            messages: VecDeque::new(),
            error: None,
        }
    }

    fn process_message(&mut self, message: Bytes) {
        self.messages.push_back(message.into());
    }

    fn process_slice(&mut self, slice: Slice, current_time: Duration) {
        let slice_constructor = self
            .slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.slices.remove(&slice.message_id);
                self.slices_last_received.remove(&slice.message_id);
                self.messages.push_back(message);
            }
            Ok(None) => {
                self.slices_last_received.insert(slice.message_id, current_time);
            }
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        self.messages.pop_front()
    }
}

struct ReceiveChannelUnreliableSequenced {
    messages: BTreeMap<u64, Bytes>,
    slices: HashMap<u64, SliceConstructor>,
    slices_last_received: HashMap<u64, Duration>,
    next_message_id: u64,
    error: Option<ChannelError>,
}

impl ReceiveChannelUnreliableSequenced {
    fn new() -> Self {
        Self {
            slices: HashMap::new(),
            slices_last_received: HashMap::new(),
            messages: BTreeMap::new(),
            next_message_id: 0,
            error: None,
        }
    }

    fn process_message(&mut self, message: Bytes, message_id: u64) {
        if message_id < self.next_message_id {
            // Drop old message
            return;
        }

        self.messages.insert(message_id, message.into());
    }

    fn process_slice(&mut self, slice: Slice, current_time: Duration) {
        if slice.message_id < self.next_message_id {
            // Drop old message
            return;
        }

        let slice_constructor = self
            .slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.slices.remove(&slice.message_id);
                self.slices_last_received.remove(&slice.message_id);
                self.messages.insert(slice.message_id, message);
            }
            Ok(None) => {
                self.slices_last_received.insert(slice.message_id, current_time);
            }
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        let Some((message_id, message)) = self.messages.pop_first() else {
            return None;    
        };

        // Remove any old slices still being assembled 
        for i in self.next_message_id..message_id {
            self.slices.remove(&i);
            self.slices_last_received.remove(&i);
        }

        self.next_message_id = message_id + 1;
        Some(message)
    }
}


struct ReceiveChannelReliable {
    slices: HashMap<u64, SliceConstructor>,
    messages: BTreeMap<u64, Bytes>,
    next_message_id: u64,
    reliable_order: ReliableOrder,
    error: Option<ChannelError>,
}

impl ReceiveChannelReliable {
    fn new() -> Self {
        Self {
            slices: HashMap::new(),
            messages: BTreeMap::new(),
            next_message_id: 0,
            reliable_order: ReliableOrder::Ordered,
            error: None,
        }
    }

    fn process_message(&mut self, message: Bytes, message_id: u64) {
        match &mut self.reliable_order {
            ReliableOrder::Ordered => {
                if !self.messages.contains_key(&message_id) {
                    self.messages.insert(message_id, message);
                }
            },
            ReliableOrder::Unordered { most_recent_message_id, received_messages } => {
                if *most_recent_message_id < message_id {
                    *most_recent_message_id = message_id;
                }
                if !received_messages.contains(&message_id) {
                    received_messages.insert(message_id);
                    self.messages.insert(message_id, message);
                }
            },
        }
    }

    fn process_reliable_slice(&mut self, slice: Slice) {
        if self.messages.contains_key(&slice.message_id) || slice.message_id < self.next_message_id {
            // Message already assembled
            return;
        }

        let slice_constructor = self
            .slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.process_message(message, slice.message_id);
            }
            Ok(None) => {}
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        match &mut self.reliable_order {
            ReliableOrder::Ordered => {
                let next_message_id = self.next_message_id;
                if !self.messages.contains_key(&next_message_id) {
                    return None;
                }

                self.next_message_id += 1;
                self.messages.remove(&next_message_id)
            }
            ReliableOrder::Unordered { received_messages, .. } => {
                let Some((message_id, message)) = self.messages.pop_first() else {
                    return None;
                };
                
                if self.next_message_id == message_id {
                    while received_messages.contains(&self.next_message_id) {
                        received_messages.remove(&message_id);
                        self.next_message_id += 1;
                    }
                }
                
                Some(message)
            }
        }
    }
}