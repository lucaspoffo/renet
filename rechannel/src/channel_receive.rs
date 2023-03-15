use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    another_channel::{Slice, SmallMessage, SLICE_SIZE},
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

struct ReceiveChannel {
    unreliable_messages: VecDeque<Bytes>,
    unreliable_slices: HashMap<u64, SliceConstructor>,
    unreliable_slices_last_received: HashMap<u64, Duration>,
    reliable_slices: HashMap<u64, SliceConstructor>,
    reliable_messages: BTreeMap<u64, Bytes>,
    next_reliable_message_id: u64,
    oldest_unacked_packet: u64,
    reliable_order: ReliableOrder,
    error: Option<ChannelError>,
}

impl SliceConstructor {
    fn new(message_id: u64, num_slices: usize) -> Self {
        SliceConstructor {
            message_id,
            num_slices: num_slices,
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

impl ReceiveChannel {
    fn new() -> Self {
        Self {
            unreliable_slices: HashMap::new(),
            unreliable_slices_last_received: HashMap::new(),
            unreliable_messages: VecDeque::new(),
            reliable_slices: HashMap::new(),
            reliable_messages: BTreeMap::new(),
            next_reliable_message_id: 0,
            oldest_unacked_packet: 0,
            reliable_order: ReliableOrder::Ordered,
            error: None,
        }
    }

    fn process_message(&mut self, message: SmallMessage, current_time: Duration) {
        match message {
            SmallMessage::Reliable { message_id, payload } => {
                if !self.reliable_messages.contains_key(&message_id) {
                    self.reliable_messages.insert(message_id, payload.into());
                }
            }
            SmallMessage::Unreliable { payload } => {
                self.unreliable_messages.push_back(payload.into());
            }
        }
    }

    fn process_reliable_slice(&mut self, slice: Slice, current_time: Duration) {
        if self.reliable_messages.contains_key(&slice.message_id) || slice.message_id < self.next_reliable_message_id {
            // Message already assembled
            return;
        }

        let slice_constructor = self
            .reliable_slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.reliable_slices.remove(&slice.message_id);
                self.reliable_messages.insert(slice.message_id, message);
            }
            Ok(None) => {}
        }
    }

    fn process_unreliable_slice(&mut self, slice: Slice, current_time: Duration) {
        let slice_constructor = self
            .unreliable_slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.unreliable_slices.remove(&slice.message_id);
                self.unreliable_slices_last_received.remove(&slice.message_id);
                self.unreliable_messages.push_back(message);
            }
            Ok(None) => {
                self.unreliable_slices_last_received.insert(slice.message_id, current_time);
            }
        }
    }

    pub fn receive_unreliable(&mut self) -> Option<Bytes> {
        self.unreliable_messages.pop_front()
    }

    pub fn receive_reliable(&mut self) -> Option<Bytes> {
        match self.reliable_order {
            ReliableOrder::Ordered => {
                let next_message_id = self.next_reliable_message_id;
                if !self.reliable_messages.contains_key(&next_message_id) {
                    return None;
                }

                self.next_reliable_message_id += 1;
                self.reliable_messages.remove(&next_message_id)
            }
            ReliableOrder::Unordered { .. } => {
                let (_, message) = self.reliable_messages.pop_first().unzip();
                message
            }
        }
    }
}

struct ReceiveChannelUnreliable {
    unreliable_messages: VecDeque<Bytes>,
    unreliable_slices: HashMap<u64, SliceConstructor>,
    unreliable_slices_last_received: HashMap<u64, Duration>,
    error: Option<ChannelError>,
}

impl ReceiveChannelUnreliable {
    fn new() -> Self {
        Self {
            unreliable_slices: HashMap::new(),
            unreliable_slices_last_received: HashMap::new(),
            unreliable_messages: VecDeque::new(),
            error: None,
        }
    }

    fn process_message(&mut self, message: Bytes) {
        self.unreliable_messages.push_back(message.into());
    }

    fn process_slice(&mut self, slice: Slice, current_time: Duration) {
        let slice_constructor = self
            .unreliable_slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.unreliable_slices.remove(&slice.message_id);
                self.unreliable_slices_last_received.remove(&slice.message_id);
                self.unreliable_messages.push_back(message);
            }
            Ok(None) => {
                self.unreliable_slices_last_received.insert(slice.message_id, current_time);
            }
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        self.unreliable_messages.pop_front()
    }
}

struct ReceiveChannelUnreliableSequenced {
    unreliable_messages: BTreeMap<u64, Bytes>,
    unreliable_slices: HashMap<u64, SliceConstructor>,
    unreliable_slices_last_received: HashMap<u64, Duration>,
    next_message_id: u64,
    error: Option<ChannelError>,
}

impl ReceiveChannelUnreliableSequenced {
    fn new() -> Self {
        Self {
            unreliable_slices: HashMap::new(),
            unreliable_slices_last_received: HashMap::new(),
            unreliable_messages: BTreeMap::new(),
            next_message_id: 0,
            error: None,
        }
    }

    fn process_message(&mut self, message: Bytes, message_id: u64) {
        if message_id < self.next_message_id {
            // Drop old message
            return;
        }

        self.unreliable_messages.insert(message_id, message.into());
    }

    fn process_slice(&mut self, slice: Slice, current_time: Duration) {
        if slice.message_id < self.next_message_id {
            // Drop old message
            return;
        }

        let slice_constructor = self
            .unreliable_slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.unreliable_slices.remove(&slice.message_id);
                self.unreliable_slices_last_received.remove(&slice.message_id);
                self.unreliable_messages.insert(slice.message_id, message);
            }
            Ok(None) => {
                self.unreliable_slices_last_received.insert(slice.message_id, current_time);
            }
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        let Some((message_id, message)) = self.unreliable_messages.pop_first() else {
            return None;    
        };

        // Remove any old slices still being assembled 
        for i in self.next_message_id..message_id {
            self.unreliable_slices.remove(&i);
            self.unreliable_slices_last_received.remove(&i);
        }

        self.next_message_id = message_id + 1;
        Some(message)
    }
}


struct ReceiveChannelReliable {
    reliable_slices: HashMap<u64, SliceConstructor>,
    reliable_messages: BTreeMap<u64, Bytes>,
    next_reliable_message_id: u64,
    oldest_unacked_packet: u64,
    reliable_order: ReliableOrder,
    error: Option<ChannelError>,
}

impl ReceiveChannelReliable {
    fn new() -> Self {
        Self {
            reliable_slices: HashMap::new(),
            reliable_messages: BTreeMap::new(),
            next_reliable_message_id: 0,
            oldest_unacked_packet: 0,
            reliable_order: ReliableOrder::Ordered,
            error: None,
        }
    }

    fn process_message(&mut self, message: Bytes, message_id: u64) {
                if !self.reliable_messages.contains_key(&message_id) {
                    self.reliable_messages.insert(message_id, message);
                }
    }

    fn process_reliable_slice(&mut self, slice: Slice, current_time: Duration) {
        if self.reliable_messages.contains_key(&slice.message_id) || slice.message_id < self.next_reliable_message_id {
            // Message already assembled
            return;
        }

        let slice_constructor = self
            .reliable_slices
            .entry(slice.message_id)
            .or_insert_with(|| SliceConstructor::new(slice.message_id, slice.num_slices));

        match slice_constructor.process_slice(slice.slice_index, &slice.payload) {
            Err(e) => self.error = Some(e),
            Ok(Some(message)) => {
                self.reliable_slices.remove(&slice.message_id);
                self.reliable_messages.insert(slice.message_id, message);
            }
            Ok(None) => {}
        }
    }

    pub fn receive_message(&mut self) -> Option<Bytes> {
        match self.reliable_order {
            ReliableOrder::Ordered => {
                let next_message_id = self.next_reliable_message_id;
                if !self.reliable_messages.contains_key(&next_message_id) {
                    return None;
                }

                self.next_reliable_message_id += 1;
                self.reliable_messages.remove(&next_message_id)
            }
            ReliableOrder::Unordered { .. } => {
                let (_, message) = self.reliable_messages.pop_first().unzip();
                message
            }
        }
    }
}