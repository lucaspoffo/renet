use std::{collections::VecDeque, mem, time::Duration};

use bincode::Options;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::{
    error::ChannelError,
    packet::{ChannelPacketData, Payload},
    sequence_buffer::SequenceBuffer,
    timer::Timer,
};
use log::{debug, error, info};

use super::{ReceiveChannel, SendChannel};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SliceMessage {
    chunk_id: u16,
    slice_id: u32,
    num_slices: u32,
    data: Payload,
}

#[derive(Debug, Clone)]
struct PacketSent {
    acked: bool,
    chunk_id: u16,
    slice_ids: Vec<u32>,
}

/// Configuration for a block channel, used for sending big and reliable messages,
/// that are not so frequent. Level initialization as an example. Messages are sent one at a time.
#[derive(Debug, Clone)]
pub struct BlockChannelConfig {
    /// Channel identifier, unique between all channels
    pub channel_id: u8,
    /// Data is sliced up into fragments of this size (bytes)
    pub slice_size: usize,
    /// Delay to wait before resending messages
    pub resend_time: Duration,
    /// Number of packet entries in the sent packet sequence buffer.
    /// Consider a few seconds of worth of entries in this buffer, based on your packet send rate
    pub sent_packet_buffer_size: usize,
    /// Maximum nuber of bytes that this channel is allowed to write per packet
    pub packet_budget: u64,
    /// Maximum size that a message can have in this channel, for the block channel this value
    /// can be above the packet budget
    pub max_message_size: u64,
    /// Queue size for the block channel.
    pub message_send_queue_size: usize,
}

#[derive(Debug)]
enum Sending {
    Yes {
        num_slices: usize,
        current_slice_id: usize,
        num_acked_slices: usize,
        acked: Vec<bool>,
        data: Bytes,
        resend_timers: Vec<Timer>,
    },
    No,
}

#[derive(Debug)]
pub struct SendBlockChannel {
    channel_id: u8,
    chunk_id: u16,
    sending: Sending,
    slice_size: usize,
    resend_time: Duration,
    packet_budget: u64,
    max_message_size: u64,
    message_send_queue_size: usize,
    packets_sent: SequenceBuffer<PacketSent>,
    messages_to_send: VecDeque<Bytes>,
    error: Option<ChannelError>,
}

#[derive(Debug)]
enum Receiving {
    Yes {
        chunk_id: u16,
        num_slices: usize,
        num_received_slices: usize,
        received: Vec<bool>,
        chunk_data: Payload,
    },
    No,
}

#[derive(Debug)]
pub struct ReceiveBlockChannel {
    channel_id: u8,
    receiving: Receiving,
    messages_received: VecDeque<Payload>,
    slice_size: usize,
    max_message_size: u64,
    error: Option<ChannelError>,
}

impl Default for BlockChannelConfig {
    fn default() -> Self {
        Self {
            channel_id: 2,
            slice_size: 400,
            resend_time: Duration::from_millis(300),
            sent_packet_buffer_size: 256,
            packet_budget: 8 * 1024,
            max_message_size: 256 * 1024,
            message_send_queue_size: 8,
        }
    }
}

impl PacketSent {
    fn new(chunk_id: u16, slice_ids: Vec<u32>) -> Self {
        Self {
            chunk_id,
            slice_ids,
            acked: false,
        }
    }
}

impl SendBlockChannel {
    pub fn new(config: BlockChannelConfig) -> Self {
        assert!((config.slice_size as u64) <= config.packet_budget);

        Self {
            chunk_id: 0,
            max_message_size: config.max_message_size,
            slice_size: config.slice_size,
            packet_budget: config.packet_budget,
            resend_time: config.resend_time,
            channel_id: config.channel_id,
            message_send_queue_size: config.message_send_queue_size,
            sending: Sending::No,
            packets_sent: SequenceBuffer::with_capacity(config.sent_packet_buffer_size),
            messages_to_send: VecDeque::with_capacity(config.message_send_queue_size),
            error: None,
        }
    }

    fn generate_slice_packets(&mut self, mut available_bytes: u64) -> Result<Vec<SliceMessage>, bincode::Error> {
        let mut slice_messages: Vec<SliceMessage> = vec![];
        match &mut self.sending {
            Sending::No => Ok(slice_messages),
            Sending::Yes {
                num_slices,
                current_slice_id,
                acked,
                resend_timers,
                data,
                ..
            } => {
                available_bytes = available_bytes.min(self.packet_budget);

                for i in 0..*num_slices {
                    let slice_id = (*current_slice_id + i) % *num_slices;

                    if acked[slice_id] {
                        continue;
                    }
                    let resend_timer = &mut resend_timers[slice_id];
                    if !resend_timer.is_finished() {
                        continue;
                    }

                    let start = slice_id * self.slice_size;
                    let end = if slice_id == *num_slices - 1 { data.len() } else { (slice_id + 1) * self.slice_size };

                    let data = data[start..end].to_vec();

                    let message = SliceMessage {
                        chunk_id: self.chunk_id,
                        slice_id: slice_id as u32,
                        num_slices: *num_slices as u32,
                        data,
                    };

                    let message_size = bincode::options().serialized_size(&message)?;
                    let message_size = message_size as u64;

                    if available_bytes < message_size {
                        break;
                    }

                    available_bytes -= message_size;
                    resend_timer.reset();

                    info!(
                        "Generated SliceMessage {} from chunk_id {}. ({}/{})",
                        message.slice_id, self.chunk_id, message.slice_id, num_slices
                    );

                    slice_messages.push(message);
                }
                *current_slice_id = (*current_slice_id + slice_messages.len()) % *num_slices;

                Ok(slice_messages)
            }
        }
    }
}

impl SendChannel for SendBlockChannel {
    fn get_messages_to_send(&mut self, available_bytes: u64, sequence: u16) -> Option<ChannelPacketData> {
        if let Sending::No = self.sending {
            if let Some(message) = self.messages_to_send.pop_front() {
                self.send_message(message);
            }
        }

        let slice_messages: Vec<SliceMessage> = match self.generate_slice_packets(available_bytes) {
            Ok(messages) => messages,
            Err(e) => {
                log::error!("Failed serialize message in block channel {}: {}", self.channel_id, e);
                self.error = Some(ChannelError::FailedToSerialize);
                return None;
            }
        };

        if slice_messages.is_empty() {
            return None;
        }

        let mut messages = vec![];
        let mut slice_ids = vec![];
        for message in slice_messages.iter() {
            let slice_id = message.slice_id;
            match bincode::options().serialize(message) {
                Ok(message) => {
                    slice_ids.push(slice_id);
                    messages.push(message);
                }
                Err(e) => {
                    error!("Failed to serialize message in block message {}: {}", self.channel_id, e);
                    self.error = Some(ChannelError::FailedToSerialize);
                    return None;
                }
            }
        }

        let packet_sent = PacketSent::new(self.chunk_id, slice_ids);
        self.packets_sent.insert(sequence, packet_sent);

        Some(ChannelPacketData {
            channel_id: self.channel_id,
            messages,
        })
    }

    fn process_ack(&mut self, ack: u16) {
        match &mut self.sending {
            Sending::No => {}
            Sending::Yes {
                num_acked_slices,
                num_slices,
                acked,
                ..
            } => {
                if let Some(sent_packet) = self.packets_sent.get_mut(ack) {
                    if sent_packet.acked || sent_packet.chunk_id != self.chunk_id {
                        return;
                    }
                    sent_packet.acked = true;

                    for &slice_id in sent_packet.slice_ids.iter() {
                        if !acked[slice_id as usize] {
                            acked[slice_id as usize] = true;
                            *num_acked_slices += 1;
                            info!(
                                "Acked SliceMessage {} from chunk_id {}. ({}/{})",
                                slice_id, self.chunk_id, num_acked_slices, num_slices
                            );
                        }
                    }

                    if num_acked_slices == num_slices {
                        self.sending = Sending::No;
                        info!("Finished sending block message {}.", self.chunk_id);
                        self.chunk_id += 1;
                    }
                }
            }
        }
    }

    fn advance_time(&mut self, duration: Duration) {
        if let Sending::Yes { resend_timers, .. } = &mut self.sending {
            for timer in resend_timers.iter_mut() {
                timer.advance(duration);
            }
        }
    }

    fn send_message(&mut self, payload: Bytes) {
        if self.error.is_some() {
            return;
        }

        if payload.len() as u64 > self.max_message_size {
            log::error!(
                "Tried to send block message with size above the limit, got {} bytes, expected less than {}",
                payload.len(),
                self.max_message_size
            );
            self.error = Some(ChannelError::SentMessageAboveMaxSize);
            return;
        }

        if matches!(self.sending, Sending::Yes { .. }) {
            if self.messages_to_send.len() >= self.message_send_queue_size {
                log::error!(
                    "Tried to send block message but the message queue is full, the limit is {} messages.",
                    self.message_send_queue_size
                );
                self.error = Some(ChannelError::SendQueueFull);
                return;
            }
            self.messages_to_send.push_back(payload);
            return;
        }

        let num_slices = (payload.len() + self.slice_size - 1) / self.slice_size;
        let mut resend_timer = Timer::new(self.resend_time);
        resend_timer.finish();
        let mut resend_timers = Vec::with_capacity(num_slices);
        resend_timers.resize(num_slices, resend_timer);

        self.sending = Sending::Yes {
            current_slice_id: 0,
            num_acked_slices: 0,
            acked: vec![false; num_slices],
            num_slices,
            resend_timers,
            data: payload,
        };
    }

    fn can_send_message(&self) -> bool {
        self.messages_to_send.len() < self.message_send_queue_size
    }

    fn error(&self) -> Option<ChannelError> {
        self.error
    }
}

impl ReceiveBlockChannel {
    pub fn new(config: BlockChannelConfig) -> Self {
        assert!((config.slice_size as u64) <= config.packet_budget);

        Self {
            slice_size: config.slice_size,
            max_message_size: config.max_message_size,
            channel_id: config.channel_id,
            receiving: Receiving::No,
            messages_received: VecDeque::new(),
            error: None,
        }
    }

    fn process_slice_message(&mut self, message: &SliceMessage) -> Result<Option<Payload>, ChannelError> {
        if matches!(self.receiving, Receiving::No) {
            if message.num_slices == 0 {
                error!("Cannot initialize block message with zero slices.");
                return Err(ChannelError::InvalidSliceMessage);
            }

            let total_size = message.num_slices as u64 * self.slice_size as u64;
            if total_size > self.max_message_size {
                error!(
                    "Cannot initialize block message above the channel limit size, got {}, expected less than {}",
                    total_size, self.max_message_size
                );
                return Err(ChannelError::ReceivedMessageAboveMaxSize);
            }

            let num_slices = message.num_slices as usize;

            self.receiving = Receiving::Yes {
                num_slices,
                chunk_id: message.chunk_id,
                num_received_slices: 0,
                received: vec![false; num_slices],
                chunk_data: vec![0; num_slices * self.slice_size],
            };
            info!(
                "Receiving Block message with id {} with {} slices.",
                message.chunk_id, message.num_slices
            );
        }

        match &mut self.receiving {
            Receiving::No => unreachable!(),
            Receiving::Yes {
                chunk_id,
                num_slices,
                chunk_data,
                received,
                num_received_slices,
            } => {
                if message.chunk_id != *chunk_id {
                    debug!(
                        "Invalid chunk id for SliceMessage, expected {}, got {}.",
                        chunk_id, message.chunk_id
                    );
                    // Not an error since this could be an old chunk id.
                    return Ok(None);
                }

                if message.num_slices != *num_slices as u32 {
                    error!(
                        "Invalid number of slices for SliceMessage, got {}, expected {}.",
                        message.num_slices, num_slices,
                    );
                    return Err(ChannelError::InvalidSliceMessage);
                }

                let slice_id = message.slice_id as usize;
                let is_last_slice = slice_id == *num_slices - 1;
                if is_last_slice {
                    if message.data.len() > self.slice_size {
                        error!(
                            "Invalid last slice_size for SliceMessage, got {}, expected less than {}.",
                            message.data.len(),
                            self.slice_size,
                        );
                        return Err(ChannelError::InvalidSliceMessage);
                    }
                } else if message.data.len() != self.slice_size {
                    error!(
                        "Invalid slice_size for SliceMessage, expected {}, got {}.",
                        self.slice_size,
                        message.data.len()
                    );
                    return Err(ChannelError::InvalidSliceMessage);
                }

                if !received[slice_id] {
                    received[slice_id] = true;
                    *num_received_slices += 1;

                    if is_last_slice {
                        let len = (*num_slices - 1) * self.slice_size + message.data.len();
                        chunk_data.resize(len, 0);
                    }

                    let start = slice_id * self.slice_size;
                    let end = if slice_id == *num_slices - 1 {
                        (*num_slices - 1) * self.slice_size + message.data.len()
                    } else {
                        (slice_id + 1) * self.slice_size
                    };

                    chunk_data[start..end].copy_from_slice(&message.data);
                    info!(
                        "Received slice {} from chunk {}. ({}/{})",
                        slice_id, chunk_id, num_received_slices, num_slices
                    );
                }

                if *num_received_slices == *num_slices {
                    info!("Received all slices for chunk {}.", chunk_id);
                    let block = mem::take(chunk_data);
                    self.receiving = Receiving::No;
                    return Ok(Some(block));
                }

                Ok(None)
            }
        }
    }
}

impl ReceiveChannel for ReceiveBlockChannel {
    fn process_messages(&mut self, messages: Vec<Payload>) {
        if self.error.is_some() {
            return;
        }

        for message in messages.iter() {
            match bincode::options().deserialize::<SliceMessage>(message) {
                Ok(slice_message) => match self.process_slice_message(&slice_message) {
                    Ok(Some(message)) => self.messages_received.push_back(message),
                    Ok(None) => {}
                    Err(e) => {
                        self.error = Some(e);
                        return;
                    }
                },
                Err(e) => {
                    error!("Failed to deserialize slice message in channel {}: {}", self.channel_id, e);
                    self.error = Some(ChannelError::FailedToSerialize);
                    return;
                }
            }
        }
    }

    fn receive_message(&mut self) -> Option<Payload> {
        self.messages_received.pop_front()
    }

    fn error(&self) -> Option<ChannelError> {
        self.error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_chunk() {
        const SLICE_SIZE: usize = 10;
        let config = BlockChannelConfig {
            slice_size: SLICE_SIZE,
            packet_budget: 30,
            ..Default::default()
        };
        let mut send_channel = SendBlockChannel::new(config.clone());
        let mut receive_channel = ReceiveBlockChannel::new(config);
        let message = Bytes::from(vec![255u8; 30]);
        send_channel.send_message(message.clone());

        let slice_messages = send_channel.generate_slice_packets(u64::MAX).unwrap();
        assert_eq!(slice_messages.len(), 2);
        send_channel.process_ack(0);
        send_channel.process_ack(1);

        for slice_message in slice_messages.into_iter() {
            receive_channel.process_slice_message(&slice_message).unwrap();
        }

        let last_message = send_channel.generate_slice_packets(u64::MAX).unwrap();
        let result = receive_channel.process_slice_message(&last_message[0]);
        assert_eq!(message, result.unwrap().unwrap());
    }

    #[test]
    fn block_chunk() {
        let config = BlockChannelConfig::default();
        let mut send_channel = SendBlockChannel::new(config.clone());
        let mut receive_channel = ReceiveBlockChannel::new(config);

        let payload = Bytes::from(vec![7u8; 102400]);

        send_channel.send_message(payload.clone());
        let mut sequence = 0;

        loop {
            let channel_data = send_channel.get_messages_to_send(1600, sequence);
            match channel_data {
                None => break,
                Some(data) => {
                    receive_channel.process_messages(data.messages);
                    send_channel.process_ack(sequence);
                    sequence += 1;
                }
            }
        }

        let received_payload = receive_channel.receive_message().unwrap();
        assert_eq!(payload, received_payload);
    }

    #[test]
    fn block_channel_queue() {
        let config = BlockChannelConfig {
            resend_time: Duration::ZERO,
            ..Default::default()
        };
        let mut send_channel = SendBlockChannel::new(config.clone());
        let mut receive_channel = ReceiveBlockChannel::new(config);

        let first_message = Bytes::from(vec![3; 2000]);
        let second_message = Bytes::from(vec![5; 2000]);
        send_channel.send_message(first_message.clone());
        send_channel.send_message(second_message.clone());

        // First message
        let block_channel_data = send_channel.get_messages_to_send(u64::MAX, 0).unwrap();
        assert!(!block_channel_data.messages.is_empty());
        receive_channel.process_messages(block_channel_data.messages);
        let received_first_message = receive_channel.receive_message().unwrap();
        assert_eq!(first_message, received_first_message);
        send_channel.process_ack(0);

        // Second message
        let block_channel_data = send_channel.get_messages_to_send(u64::MAX, 1).unwrap();
        assert!(!block_channel_data.messages.is_empty());
        receive_channel.process_messages(block_channel_data.messages);
        let received_second_message = receive_channel.receive_message().unwrap();
        assert_eq!(second_message, received_second_message);
        send_channel.process_ack(1);

        // Check there is no message to send
        assert!(matches!(send_channel.sending, Sending::No));
    }

    #[test]
    fn acking_packet_with_old_chunk_id() {
        let config = BlockChannelConfig {
            resend_time: Duration::ZERO,
            ..Default::default()
        };
        let mut send_channel = SendBlockChannel::new(config);
        let first_message = Bytes::from(vec![5; 400 * 3]);
        let second_message = Bytes::from(vec![3; 400]);
        send_channel.send_message(first_message);
        send_channel.send_message(second_message);

        let _ = send_channel.get_messages_to_send(u64::MAX, 0).unwrap();
        let _ = send_channel.get_messages_to_send(u64::MAX, 1).unwrap();

        send_channel.process_ack(0);
        let _ = send_channel.get_messages_to_send(u64::MAX, 2).unwrap();

        send_channel.process_ack(1);
        assert!(matches!(send_channel.sending, Sending::Yes { .. }));

        send_channel.process_ack(2);
        assert!(matches!(send_channel.sending, Sending::No));
    }

    #[test]
    fn initialize_block_with_zero_slices() {
        let mut receive_channel = ReceiveBlockChannel::new(Default::default());
        let slice_message = SliceMessage {
            chunk_id: 0,
            slice_id: 0,
            num_slices: 0,
            data: vec![],
        };
        assert!(receive_channel.process_slice_message(&slice_message).is_err());
        assert!(matches!(receive_channel.receiving, Receiving::No));
    }
}
