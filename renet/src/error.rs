use rechannel::error::DisconnectionReason as RechannelDisconnectReason;
use renetcode::DisconnectReason as NetcodeDisconnectReason;

use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum RenetError {
    NetcodeError(renetcode::NetcodeError),
    RechannelError(rechannel::error::RechannelError),
    IOError(std::io::Error),
}

impl Error for RenetError {}

impl fmt::Display for RenetError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RenetError::NetcodeError(ref err) => err.fmt(fmt),
            RenetError::RechannelError(ref err) => err.fmt(fmt),
            RenetError::IOError(ref err) => err.fmt(fmt),
        }
    }
}

impl From<renetcode::NetcodeError> for RenetError {
    fn from(inner: renetcode::NetcodeError) -> Self {
        RenetError::NetcodeError(inner)
    }
}

impl From<rechannel::error::RechannelError> for RenetError {
    fn from(inner: rechannel::error::RechannelError) -> Self {
        RenetError::RechannelError(inner)
    }
}

impl From<std::io::Error> for RenetError {
    fn from(inner: std::io::Error) -> Self {
        RenetError::IOError(inner)
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
