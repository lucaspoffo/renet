use std::time::Instant;
// TODO: Remove bincode and serde dependency
use serde::{Deserialize, Serialize};

mod reliable;
pub use reliable::{ReliableOrderedChannel, ReliableOrderedChannelConfig};

mod unreliable;
pub use unreliable::{UnreliableUnorderedChannel, UnreliableUnorderedChannelConfig};

pub trait ChannelConfig {
    fn new_channel(&self) -> Box<dyn Channel>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    id: u16,
    payload: Box<[u8]>,
}

impl Message {
    pub fn new(id: u16, payload: Box<[u8]>) -> Self {
        Self { id, payload }
    }

    pub fn serialized_size_bytes(&self) -> u32 {
        self.payload.len() as u32 + 2
    }
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: 0,
            payload: Box::default(),
        }
    }
}

pub trait Channel {
    fn get_messages_to_send(
        &mut self,
        available_bits: Option<u32>,
        sequence: u16,
    ) -> Option<Vec<Message>>;
    fn process_messages(&mut self, messages: Vec<Message>);
    fn process_ack(&mut self, ack: u16);
    fn send_message(&mut self, message_payload: Box<[u8]>);
    fn receive_message(&mut self) -> Option<Box<[u8]>>;
    // TODO: do we need reset at all?
    fn reset(&mut self);
}

#[derive(Debug, Clone)]
pub(crate) struct MessageSend {
    message: Message,
    last_time_sent: Option<Instant>,
    serialized_size_bits: u32,
}

impl MessageSend {
    pub fn new(message: Message) -> Self {
        Self {
            serialized_size_bits: message.serialized_size_bytes() * 8,
            message,
            last_time_sent: None,
        }
    }
}

impl Default for MessageSend {
    fn default() -> Self {
        Self {
            message: Message::default(),
            last_time_sent: None,
            serialized_size_bits: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PacketSent {
    acked: bool,
    time_sent: Instant,
    messages_id: Vec<u16>,
}

impl PacketSent {
    pub fn new(messages_id: Vec<u16>) -> Self {
        Self {
            acked: false,
            time_sent: Instant::now(),
            messages_id,
        }
    }
}

impl Default for PacketSent {
    fn default() -> Self {
        Self {
            acked: false,
            time_sent: Instant::now(),
            messages_id: vec![],
        }
    }
}

// TODO: Change Vec<Message> -> Vec<Vec<u8>> only reliable message uses Message for the message_id
#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelPacketData {
    pub(crate) messages: Vec<Message>,
    pub(crate) channel_id: u8,
}

impl ChannelPacketData {
    pub fn new(messages: Vec<Message>, channel_id: u8) -> Self {
        Self {
            messages,
            channel_id,
        }
    }
}
