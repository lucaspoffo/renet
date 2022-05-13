mod client;
mod error;
mod server;

pub use rechannel::channel::{BlockChannelConfig, ChannelConfig, ReliableChannelConfig, UnreliableChannelConfig};
pub use rechannel::error::{ChannelError, DisconnectionReason, RechannelError};
pub use rechannel::remote_connection::NetworkInfo;

pub use renetcode::{ConnectToken, NetcodeError};
pub use renetcode::{NETCODE_KEY_BYTES, NETCODE_MAX_PAYLOAD_BYTES, NETCODE_USER_DATA_BYTES};

pub use client::RenetClient;
pub use error::RenetError;
pub use server::{RenetServer, ServerConfig, ServerEvent};

use std::time::Duration;

use rechannel::{remote_connection::ConnectionConfig, FragmentConfig};

const NUM_DISCONNECT_PACKETS_TO_SEND: u32 = 5;

/// Configuration for a renet connection and its channels.
pub struct RenetConnectionConfig {
    /// The maximum size (bytes) that generated packets can have.
    pub max_packet_size: u64,
    /// Number of sent packets to keep track while calculating metrics.
    pub sent_packets_buffer_size: usize,
    /// Number of received packets to keep track while calculating metrics.
    pub received_packets_buffer_size: usize,
    /// Size of the buffer that queues up fragments ready to be reassembled once all fragments have arrived.
    pub reassembly_buffer_size: usize,
    /// Smoothing factor for metrics: rtt, sent kbps, received kbps.
    /// Values between 0.0 and 1.0.
    pub measure_smoothing_factor: f64,
    /// Value which specifies at which interval a heartbeat should be sent, if no other packet was sent in the meantime.
    pub heartbeat_time: Duration,
    /// Per-channel configuration.
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
