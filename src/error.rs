use std::{io, result};

pub type Result<T> = result::Result<T, RenetError>;

#[derive(Debug)]
pub enum RenetError {
    MaximumFragmentsExceeded,
    MaximumPacketSizeExceeded,
    CouldNotFindFragment,
    InvalidNumberFragment,
    FragmentAlreadyProcessed,
    InvalidHeaderType,
    FragmentMissingPacketHeader,
    IOError(io::Error),
    SerializationFailed,
    AuthenticationError(Box<dyn std::error::Error>),
}

impl From<io::Error> for RenetError {
    fn from(inner: io::Error) -> RenetError {
        RenetError::IOError(inner)
    }
}

