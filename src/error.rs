use crate::reassembly_fragment::FragmentError;

use std::fmt::{self, Display, Formatter};
use std::{io, result};
use serde::{Serialize, Deserialize};

pub type Result<T> = result::Result<T, RenetError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConnectionError {
    Denied,
    MaxPlayer,
}

impl ConnectionError {
    pub fn from_u8(code: u8) -> Result<Self> {
        match code {
            1 => Ok(ConnectionError::Denied),
            2 => Ok(ConnectionError::MaxPlayer),
            _ => Err(RenetError::InvalidConnectionError)
        }
    }
}

#[derive(Debug)]
pub enum RenetError {
    MaximumFragmentsExceeded,
    MaximumPacketSizeExceeded,
    CouldNotFindFragment,
    InvalidNumberFragment,
    FragmentAlreadyProcessed,
    InvalidHeaderType,
    InvalidConnectionError,
    FragmentMissingPacketHeader,
    IOError(io::Error),
    SerializationFailed,
    AuthenticationError(Box<dyn std::error::Error>),
    ConnectionTimedOut,
    FragmentError(FragmentError),
    InvalidPacket,
    ConnectionError(ConnectionError),
}

impl From<io::Error> for RenetError {
    fn from(inner: io::Error) -> RenetError {
        RenetError::IOError(inner)
    }
}

impl From<FragmentError> for RenetError {
    fn from(inner: FragmentError) -> RenetError {
        RenetError::FragmentError(inner)
    }
}

// TODO: add comments and impl error display correctly
impl Display for RenetError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        write!(fmt, "RenetError")
    }
}
