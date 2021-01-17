use std::fmt::{self, Display, Formatter};
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
    ConnectionTimedOut,
}

impl From<io::Error> for RenetError {
    fn from(inner: io::Error) -> RenetError {
        RenetError::IOError(inner)
    }
}

// TODO: add comments and impl error display correctly
impl Display for RenetError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        write!(fmt, "RenetError")
    }
}
