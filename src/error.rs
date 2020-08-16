use std::{io, result};
use crate::connection::ConnectionError;

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
    ConnectionError(ConnectionError),
}

impl From<io::Error> for RenetError {
    fn from(inner: io::Error) -> RenetError {
        RenetError::IOError(inner)
    }
}

impl From<ConnectionError> for RenetError {
    fn from(inner: ConnectionError) -> RenetError {
        RenetError::ConnectionError(inner)
    }
}
