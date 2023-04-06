// use rechannel::{channel::ChannelConfig, remote_connection::ConnectionConfig, FragmentConfig};
use rechannel::{channels::{ChannelConfig, SendType}, remote_connection::ConnectionConfig};
use std::time::Duration;

/// Configuration for a renet connection and its channels.
// pub struct RenetConnectionConfig {
//     /// The maximum size (bytes) that generated packets can have.
//     pub max_packet_size: u64,
//     /// Number of sent packets to keep track while calculating metrics.
//     pub sent_packets_buffer_size: usize,
//     /// Number of received packets to keep track while calculating metrics.
//     pub received_packets_buffer_size: usize,
//     /// Size of the buffer that queues up fragments ready to be reassembled once all fragments have arrived.
//     pub reassembly_buffer_size: usize,
//     /// Smoothing factor for Round Time Trip.
//     /// Values between 0.0 and 1.0.
//     pub rtt_smoothing_factor: f32,
//     /// Smoothing factor for Packet Loss.
//     /// Values between 0.0 and 1.0.
//     pub packet_loss_smoothing_factor: f32,
//     /// Smoothing factor for Kbps Sent/Received.
//     /// Values between 0.0 and 1.0.
//     pub bandwidth_smoothing_factor: f32,
//     /// Value which specifies at which interval a heartbeat should be sent, if no other packet was sent in the meantime.
//     pub heartbeat_time: Duration,
//     /// Channels configuration that this client/server will use to send messages.
//     pub send_channels_config: Vec<ChannelConfig>,
//     /// Channels configuration that this client/server will use to receive messages.
//     pub receive_channels_config: Vec<ChannelConfig>,
// }

pub struct RenetConnectionConfig {
    pub send_channels_config: Vec<ChannelConfig>,
    pub receive_channels_config: Vec<ChannelConfig>,
}

impl Default for RenetConnectionConfig {
    fn default() -> Self {
        let channels_config = vec![
            ChannelConfig { channel_id: 0, max_memory_usage_bytes: 1024 * 20, send_type: SendType::Unreliable },
            ChannelConfig { channel_id: 1, max_memory_usage_bytes: 1024 * 20, send_type: SendType::ReliableUnordered { resend_time: Duration::from_millis(300) } },
            ChannelConfig { channel_id: 2, max_memory_usage_bytes: 1024 * 20, send_type: SendType::ReliableOrdered { resend_time: Duration::from_millis(300) } }

        ];

        Self {
            send_channels_config: channels_config.clone(),
            receive_channels_config: channels_config,
        }
    }
}

impl RenetConnectionConfig {
    pub fn to_connection_config(&self) -> ConnectionConfig {
        ConnectionConfig {
            send_channels_config: self.send_channels_config.clone(),
            receive_channels_config: self.receive_channels_config.clone(),
        }
    }
}
