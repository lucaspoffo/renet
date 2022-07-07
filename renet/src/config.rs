use rechannel::{channel::ChannelConfig, remote_connection::ConnectionConfig, FragmentConfig};
use renetcode::NETCODE_MAX_PAYLOAD_BYTES;

use std::time::Duration;

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
    /// Smoothing factor for Round Time Trip.
    /// Values between 0.0 and 1.0.
    pub rtt_smoothing_factor: f32,
    /// Smoothing factor for Packet Loss.
    /// Values between 0.0 and 1.0.
    pub packet_loss_smoothing_factor: f32,
    /// Smoothing factor for Kbps Sent/Received.
    /// Values between 0.0 and 1.0.
    pub bandwidth_smoothing_factor: f32,
    /// Value which specifies at which interval a heartbeat should be sent, if no other packet was sent in the meantime.
    pub heartbeat_time: Duration,
    /// Channels configuration that this client/server will use to send messages.
    pub send_channels_config: Vec<ChannelConfig>,
    /// Channels configuration that this client/server will use to receive messages.
    pub receive_channels_config: Vec<ChannelConfig>,
}

impl Default for RenetConnectionConfig {
    fn default() -> Self {
        let channels_config = vec![
            ChannelConfig::Reliable(Default::default()),
            ChannelConfig::Unreliable(Default::default()),
            ChannelConfig::Block(Default::default()),
        ];

        Self {
            max_packet_size: 16 * 1024,
            sent_packets_buffer_size: 256,
            received_packets_buffer_size: 256,
            reassembly_buffer_size: 256,
            rtt_smoothing_factor: 0.005,
            packet_loss_smoothing_factor: 0.1,
            bandwidth_smoothing_factor: 0.1,
            heartbeat_time: Duration::from_millis(100),
            send_channels_config: channels_config.clone(),
            receive_channels_config: channels_config,
        }
    }
}

impl RenetConnectionConfig {
    pub fn to_connection_config(&self) -> ConnectionConfig {
        let fragment_config = FragmentConfig {
            fragment_above: NETCODE_MAX_PAYLOAD_BYTES as u64 - 40,
            fragment_size: NETCODE_MAX_PAYLOAD_BYTES - 40,
            reassembly_buffer_size: self.reassembly_buffer_size,
        };

        ConnectionConfig {
            max_packet_size: self.max_packet_size,
            sent_packets_buffer_size: self.sent_packets_buffer_size,
            received_packets_buffer_size: self.received_packets_buffer_size,
            rtt_smoothing_factor: self.rtt_smoothing_factor,
            packet_loss_smoothing_factor: self.packet_loss_smoothing_factor,
            heartbeat_time: self.heartbeat_time,
            send_channels_config: self.send_channels_config.clone(),
            receive_channels_config: self.receive_channels_config.clone(),
            fragment_config,
        }
    }
}
