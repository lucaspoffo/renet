const MAX_MESSAGE_BATCH_SIZE: usize = 512;

mod client;
mod server;

#[cfg(feature = "bevy")]
pub mod bevy;

pub use client::SteamClientTransport;
pub use server::{AccessPermission, SteamServerConfig, SteamServerTransport};

#[doc(hidden)]
pub use steamworks;
