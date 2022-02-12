mod client;
mod server;

pub use rechannel::channel::ChannelConfig;
use rechannel::{remote_connection::ConnectionConfig, FragmentConfig};
pub use renetcode::ConnectToken;
pub use renetcode::{NETCODE_KEY_BYTES, NETCODE_MAX_PAYLOAD_BYTES, NETCODE_USER_DATA_BYTES};

pub use client::RenetClient;
pub use server::{RenetServer, ServerConfig, ServerEvent};

use std::error::Error;
use std::fmt;
use std::time::Duration;

const NUM_DISCONNECT_PACKETS_TO_SEND: u32 = 5;

#[derive(Debug)]
pub enum RenetError {
    NetcodeError(renetcode::NetcodeError),
    RechannelError(rechannel::error::RechannelError),
    IOError(std::io::Error),
}

impl Error for RenetError {}

impl fmt::Display for RenetError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RenetError::NetcodeError(ref err) => err.fmt(fmt),
            RenetError::RechannelError(ref err) => err.fmt(fmt),
            RenetError::IOError(ref err) => err.fmt(fmt),
        }
    }
}

impl From<renetcode::NetcodeError> for RenetError {
    fn from(inner: renetcode::NetcodeError) -> Self {
        RenetError::NetcodeError(inner)
    }
}

impl From<rechannel::error::RechannelError> for RenetError {
    fn from(inner: rechannel::error::RechannelError) -> Self {
        RenetError::RechannelError(inner)
    }
}

impl From<std::io::Error> for RenetError {
    fn from(inner: std::io::Error) -> Self {
        RenetError::IOError(inner)
    }
}

pub struct RenetConnectionConfig {
    pub max_packet_size: u64,
    pub sent_packets_buffer_size: usize,
    pub received_packets_buffer_size: usize,
    pub reassembly_buffer_size: usize,
    pub measure_smoothing_factor: f64,
    pub heartbeat_time: Duration,
    pub channels_config: Vec<ChannelConfig>,
}

impl Default for RenetConnectionConfig {
    fn default() -> Self {
        Self {
            max_packet_size: 16 * 1024,
            sent_packets_buffer_size: 256,
            received_packets_buffer_size: 256,
            reassembly_buffer_size: 256,
            measure_smoothing_factor: 0.1,
            heartbeat_time: Duration::from_millis(200),
            channels_config: vec![
                ChannelConfig::Reliable(Default::default()),
                ChannelConfig::Unreliable(Default::default()),
                ChannelConfig::Block(Default::default()),
            ],
        }
    }
}

impl RenetConnectionConfig {
    fn to_connection_config(&self) -> ConnectionConfig {
        let fragment_config = FragmentConfig {
            fragment_above: NETCODE_MAX_PAYLOAD_BYTES as u64 - 40,
            fragment_size: NETCODE_MAX_PAYLOAD_BYTES - 40,
            reassembly_buffer_size: self.reassembly_buffer_size,
        };

        ConnectionConfig {
            max_packet_size: self.max_packet_size,
            sent_packets_buffer_size: self.sent_packets_buffer_size,
            received_packets_buffer_size: self.received_packets_buffer_size,
            measure_smoothing_factor: self.measure_smoothing_factor,
            heartbeat_time: self.heartbeat_time,
            channels_config: self.channels_config.clone(),
            fragment_config,
        }
    }
}
