use crate::error::DisconnectionReason;

use bincode::Options;
use serde::{Deserialize, Serialize};

pub type Payload = Vec<u8>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelPacketData {
    pub(crate) messages: Vec<Payload>,
    pub(crate) channel_id: u8,
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub(crate) struct AckData {
    pub ack: u16,
    pub ack_bits: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Packet {
    Normal {
        sequence: u16,
        ack_data: AckData,
        channels_packet_data: Vec<ChannelPacketData>,
    },
    Fragment {
        sequence: u16,
        ack_data: AckData,
        fragment_data: FragmentData,
    },
    Heartbeat {
        ack_data: AckData,
    },
    Disconnect {
        reason: DisconnectionReason,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FragmentData {
    pub fragment_id: u8,
    pub num_fragments: u8,
    pub payload: Payload,
}

impl std::fmt::Debug for AckData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ack")
            .field("last ack", &self.ack)
            .field("ack mask", &format!("{:031b}", &self.ack_bits))
            .finish()
    }
}

/// Given a disconnect reason, serialize a disconnect packet to be sent.
pub fn disconnect_packet(reason: DisconnectionReason) -> Result<Payload, bincode::Error> {
    let packet = Packet::Disconnect { reason };
    let packet = bincode::options().serialize(&packet)?;
    Ok(packet)
}
