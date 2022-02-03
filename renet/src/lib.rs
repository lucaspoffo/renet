pub mod client;
pub mod server;

pub use rechannel;
use rechannel::{channel::ChannelConfig, FragmentConfig, remote_connection::ConnectionConfig};
use renetcode::NETCODE_MAX_PAYLOAD_BYTES;

use std::error::Error;
use std::fmt;
use std::time::Duration;

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

pub struct RenetConnnectionConfig {
    pub max_packet_size: u64,
    pub sent_packets_buffer_size: usize,
    pub received_packets_buffer_size: usize,
    pub reassembly_buffer_size: usize,
    pub measure_smoothing_factor: f64,
    pub heartbeat_time: Duration,
    pub channels_config: Vec<ChannelConfig>,
    pub fragment_config: FragmentConfig,
}

impl RenetConnnectionConfig {
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
