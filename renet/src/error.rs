use std::error::Error;
use std::fmt;

use rechannel::error::DisconnectionReason as RechannelDisconnectReason;
use renetcode::DisconnectReason as NetcodeDisconnectReason;

/// Enum with possibles errors that can occur.
#[derive(Debug)]
pub enum RenetError {
    Netcode(renetcode::NetcodeError),
    Rechannel(rechannel::error::RechannelError),
    IO(std::io::Error),
}

impl Error for RenetError {}

impl fmt::Display for RenetError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RenetError::Netcode(ref err) => err.fmt(fmt),
            RenetError::Rechannel(ref err) => err.fmt(fmt),
            RenetError::IO(ref err) => err.fmt(fmt),
        }
    }
}

impl From<renetcode::NetcodeError> for RenetError {
    fn from(inner: renetcode::NetcodeError) -> Self {
        RenetError::Netcode(inner)
    }
}

impl From<renetcode::TokenGenerationError> for RenetError {
    fn from(inner: renetcode::TokenGenerationError) -> Self {
        RenetError::Netcode(renetcode::NetcodeError::TokenGenerationError(inner))
    }
}

impl From<rechannel::error::RechannelError> for RenetError {
    fn from(inner: rechannel::error::RechannelError) -> Self {
        RenetError::Rechannel(inner)
    }
}

impl From<std::io::Error> for RenetError {
    fn from(inner: std::io::Error) -> Self {
        RenetError::IO(inner)
    }
}

pub enum DisconnectionReason {
    Rechannel(RechannelDisconnectReason),
    Netcode(NetcodeDisconnectReason),
}

impl From<RechannelDisconnectReason> for DisconnectionReason {
    fn from(reason: RechannelDisconnectReason) -> Self {
        DisconnectionReason::Rechannel(reason)
    }
}

impl From<NetcodeDisconnectReason> for DisconnectionReason {
    fn from(error: NetcodeDisconnectReason) -> Self {
        DisconnectionReason::Netcode(error)
    }
}

impl fmt::Display for DisconnectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DisconnectionReason::*;

        match *self {
            Rechannel(reason) => write!(f, "{}", reason),
            Netcode(error) => write!(f, "{}", error),
        }
    }
}
