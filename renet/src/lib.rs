mod circular_buffer;
mod client;
mod config;
mod error;
mod network_info;
mod server;

pub use rechannel::channel::{BlockChannelConfig, ChannelConfig, ReliableChannelConfig, UnreliableChannelConfig};
pub use rechannel::error::{ChannelError, DisconnectionReason, RechannelError};

pub use renetcode::{ConnectToken, NetcodeError};
pub use renetcode::{NETCODE_KEY_BYTES, NETCODE_USER_DATA_BYTES};

pub use client::{ClientAuthentication, RenetClient};
pub use config::RenetConnectionConfig;
pub use error::RenetError;
pub use network_info::NetworkInfo;
pub use server::{RenetServer, ServerAuthentication, ServerConfig, ServerEvent};

// Reused in the renet_visualizer crate
#[doc(hidden)]
pub use circular_buffer::CircularBuffer;

const NUM_DISCONNECT_PACKETS_TO_SEND: u32 = 5;
