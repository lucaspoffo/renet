pub mod client;
pub mod server;

pub use rechannel;

use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum RenetUdpError {
    RenetError(rechannel::error::RechannelError),
    IOError(std::io::Error),
}

impl Error for RenetUdpError {}

impl fmt::Display for RenetUdpError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RenetUdpError::RenetError(ref renet_err) => renet_err.fmt(fmt),
            RenetUdpError::IOError(ref io_err) => io_err.fmt(fmt),
        }
    }
}

impl From<rechannel::error::RechannelError> for RenetUdpError {
    fn from(inner: rechannel::error::RechannelError) -> Self {
        RenetUdpError::RenetError(inner)
    }
}

impl From<std::io::Error> for RenetUdpError {
    fn from(inner: std::io::Error) -> Self {
        RenetUdpError::IOError(inner)
    }
}
