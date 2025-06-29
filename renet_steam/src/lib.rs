const MAX_MESSAGE_BATCH_SIZE: usize = 512;

mod client;
mod server;

pub use client::SteamClientTransport;
pub use server::{AccessPermission, SteamServerConfig, SteamServerSocketOptions, SteamServerTransport};

#[doc(hidden)]
pub use steamworks;
