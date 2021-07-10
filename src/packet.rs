use crate::error::ConnectionError;

use serde::{Deserialize, Serialize};

pub type Payload = Vec<u8>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelPacketData {
    pub(crate) messages: Vec<Payload>,
    pub(crate) channel_id: u8,
}

impl ChannelPacketData {
    pub fn new(messages: Vec<Payload>, channel_id: u8) -> Self {
        Self {
            messages,
            channel_id,
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
    Unauthenticaded(Unauthenticaded),
    Authenticated(Authenticated),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Unauthenticaded {
    ConnectionError(ConnectionError),
    Protocol { payload: Payload },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Authenticated {
    pub payload: Payload,
}

impl From<Authenticated> for Packet {
    fn from(value: Authenticated) -> Self {
        Self::Authenticated(value)
    }
}

impl From<Unauthenticaded> for Packet {
    fn from(value: Unauthenticaded) -> Self {
        Self::Unauthenticaded(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Message {
    Normal(Normal),
    Fragment(Fragment),
    Heartbeat(HeartBeat),
    ConnectionError(ConnectionError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Normal {
    pub sequence: u16,
    pub ack_data: AckData,
    pub payload: Vec<ChannelPacketData>,
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

impl From<Normal> for Message {
    fn from(value: Normal) -> Self {
        Self::Normal(value)
    }
}

impl From<Fragment> for Message {
    fn from(value: Fragment) -> Self {
        Self::Fragment(value)
    }
}

impl From<HeartBeat> for Message {
    fn from(value: HeartBeat) -> Self {
        Self::Heartbeat(value)
    }
}

impl From<ConnectionError> for Message {
    fn from(value: ConnectionError) -> Self {
        Self::ConnectionError(value)
    }
}
