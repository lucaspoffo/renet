use rechannel::error::DisconnectionReason as RechannelDisconnectReason;
use renetcode::DisconnectReason as NetcodeDisconnectReason;

use std::error::Error;
use std::fmt;

/// Enum with possibles errors that can occur.
#[derive(Debug)]
pub enum RenetError {
    Netcode(renetcode::NetcodeError),
    Rechannel(rechannel::error::RechannelError),
    ReceiverDisconnected,
    SenderDisconnected,
}

impl Error for RenetError {}

impl fmt::Display for RenetError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RenetError::Netcode(ref err) => err.fmt(fmt),
            RenetError::Rechannel(ref err) => err.fmt(fmt),
            RenetError::SenderDisconnected => write!(fmt, "packet sender has been disconnected"),
            RenetError::ReceiverDisconnected => write!(fmt, "packet receiver has been disconnected"),
        }
    }
}

impl From<renetcode::NetcodeError> for RenetError {
    fn from(inner: renetcode::NetcodeError) -> Self {
        RenetError::Netcode(inner)
    }
}

impl From<rechannel::error::RechannelError> for RenetError {
    fn from(inner: rechannel::error::RechannelError) -> Self {
        RenetError::Rechannel(inner)
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
