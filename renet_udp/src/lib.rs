pub mod client;
pub mod server;

pub use renet;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenetUdpError {
    #[error("renet error: {0}")]
    RenetError(#[from] renet::error::RenetError),
    #[error("io error: {0}")]
    IOError(#[from] std::io::Error),
}
