use crate::error::ConnectionError;

use serde::{Deserialize, Serialize};

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
    Connection(Connection),
}

impl Packet {
    pub fn connection_error(error: ConnectionError) -> Self {
        return Packet::Connection(Connection { error: Some(error), payload: vec![] });
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Normal {
    pub sequence: u16,
    pub ack_data: AckData,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fragment {
    pub sequence: u16,
    pub ack_data: AckData,
    pub fragment_id: u8,
    pub num_fragments: u8,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeartBeat {
    pub ack_data: AckData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Connection {
    pub error: Option<ConnectionError>,
    pub payload: Vec<u8>,
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

impl From<Connection> for Packet {
    fn from(value: Connection) -> Self {
        Self::Connection(value)
    }
}
