const MAX_MESSAGE_BATCH_SIZE: usize = 255;

mod client;
mod server;

pub use client::SteamClientTransport;
pub use server::SteamServerTransport;

#[doc(hidden)]
pub use steamworks;