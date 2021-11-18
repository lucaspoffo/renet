use crate::channel::block::SliceMessage;
use crate::channel::reliable::ReliableMessage;
use crate::error::DisconnectionReason;

use serde::{Deserialize, Serialize};

pub type Payload = Vec<u8>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReliableChannelData {
    pub channel_id: u8,
    pub messages: Vec<ReliableMessage>,
}

impl ReliableChannelData {
    pub fn new(channel_id: u8, messages: Vec<ReliableMessage>) -> Self {
        Self {
            channel_id,
            messages,
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct AckData {
    pub ack: u16,
    pub ack_bits: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Packet {
    Normal(Normal),
    Fragment(Fragment),
    Heartbeat(HeartBeat),
    Disconnect(DisconnectionReason),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelMessages {
    pub slice_messages: Vec<SliceMessage>,
    pub unreliable_messages: Vec<Payload>,
    pub reliable_channels_data: Vec<ReliableChannelData>,
}

impl ChannelMessages {
    pub fn is_empty(&self) -> bool {
        self.slice_messages.is_empty()
            && self.unreliable_messages.is_empty()
            && self.reliable_channels_data.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Normal {
    pub sequence: u16,
    pub ack_data: AckData,
    pub channel_messages: ChannelMessages,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fragment {
    pub sequence: u16,
    pub ack_data: AckData,
    pub fragment_id: u8,
    pub num_fragments: u8,
    pub payload: Payload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeartBeat {
    pub ack_data: AckData,
}

impl From<Normal> for Packet {
    fn from(value: Normal) -> Self {
        Self::Normal(value)
    }
}

impl From<Fragment> for Packet {
    fn from(value: Fragment) -> Self {
        Self::Fragment(value)
    }
}

impl From<HeartBeat> for Packet {
    fn from(value: HeartBeat) -> Self {
        Self::Heartbeat(value)
    }
}

impl From<DisconnectionReason> for Packet {
    fn from(value: DisconnectionReason) -> Self {
        Self::Disconnect(value)
    }
}
