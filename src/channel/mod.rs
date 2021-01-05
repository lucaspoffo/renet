use std::time::{Duration, Instant};
// TODO: Remove bincode and serde dependency
use bincode;
use serde::{Deserialize, Serialize};

mod reliable;
pub use reliable::ReliableOrderedChannel;

#[derive(Clone)]
pub enum ChannelType {
    ReliableOrderedChannel,
}

#[derive(Clone)]
pub struct ChannelConfig {
    pub channel_type: ChannelType,
    pub sent_packet_buffer_size: usize,
    pub message_send_queue_size: usize,
    pub message_receive_queue_size: usize,
    pub max_message_per_packet: u32,
    pub packet_budget_bytes: Option<u32>,
    pub message_resend_time: Duration,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            channel_type: ChannelType::ReliableOrderedChannel,
            sent_packet_buffer_size: 1024,
            message_send_queue_size: 1024,
            message_receive_queue_size: 1024,
            max_message_per_packet: 256,
            packet_budget_bytes: None,
            message_resend_time: Duration::from_millis(100),
        }
    }
}

impl ChannelConfig {
    pub fn new_channel(&self, current_time: Instant) -> Box<dyn Channel> {
        match self.channel_type {
            ChannelType::ReliableOrderedChannel => {
                return Box::new(reliable::ReliableOrderedChannel::new(
                    current_time,
                    self.clone(),
                ));
            }
        }
    }
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
    fn get_packet_data(
        &mut self,
        available_bits: Option<u32>,
        sequence: u16,
    ) -> Option<ChannelPacketData>;
    fn process_packet_data(&mut self, packet_data: &ChannelPacketData);
    fn process_ack(&mut self, ack: u16);
    fn send_message(&mut self, message_payload: Box<[u8]>);
    fn receive_message(&mut self) -> Option<Box<[u8]>>;
    fn reset(&mut self);
    fn update_current_time(&mut self, time: Instant);
    fn set_id(&mut self, id: u8);
    fn id(&self) -> u8;
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
            serialized_size_bits: bincode::serialized_size(&message).unwrap() as u32 * 8,
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

#[derive(Serialize, Deserialize)]
pub struct ChannelPacketData {
    messages: Vec<Message>,
    channel_id: u8,
}

impl ChannelPacketData {
    pub fn new(messages: Vec<Message>, channel_id: u8) -> Self {
        Self {
            messages,
            channel_id,
        }
    }

    pub fn channel_id(&self) -> u8 {
        self.channel_id
    }
}
