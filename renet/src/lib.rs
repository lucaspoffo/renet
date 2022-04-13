mod client;
mod error;
mod server;

pub use rechannel::channel::ChannelConfig;
use rechannel::{remote_connection::ConnectionConfig, FragmentConfig};
pub use renetcode::ConnectToken;
pub use renetcode::{NETCODE_KEY_BYTES, NETCODE_MAX_PAYLOAD_BYTES, NETCODE_USER_DATA_BYTES};

pub use client::RenetClient;
pub use error::RenetError;
pub use server::{RenetServer, ServerConfig, ServerEvent};

use std::time::Duration;

const NUM_DISCONNECT_PACKETS_TO_SEND: u32 = 5;

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
            heartbeat_time: Duration::from_millis(100),
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
