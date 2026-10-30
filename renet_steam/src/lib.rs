const DEFAULT_MAX_MESSAGE_BATCH_SIZE: usize = 1000;

mod client;
mod server;

pub use client::{SteamClientTransport, SteamClientTransportConfig};
pub use server::{AccessPermission, SteamServerConfig, SteamServerSocketOptions, SteamServerTransport};

#[doc(hidden)]
pub use steamworks;
