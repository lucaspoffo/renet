use std::{error, fmt, io};

use crate::{token::TokenGenerationError, NETCODE_MAX_PAYLOAD_BYTES};
use aead::Error as CryptoError;

#[derive(Debug)]
pub enum NetcodeError {
    InvalidPrivateKey,
    InvalidPacketType,
    InvalidProtocolID,
    InvalidVersion,
    PacketTooSmall,
    PayloadAboveLimit,
    DuplicatedSequence,
    NoMoreServers,
    Expired,
    TimedOut,
    Disconnected,
    CryptoError,
    NotInHostList,
    BufferTooSmall,
    ClientNotFound,
    ClientNotConnected,
    IoError(io::Error),
    TokenGenerationError(TokenGenerationError),
}

impl fmt::Display for NetcodeError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use NetcodeError::*;

        match *self {
            InvalidPrivateKey => write!(fmt, "invalid private key"),
            InvalidPacketType => write!(fmt, "invalid packet type"),
            InvalidProtocolID => write!(fmt, "invalid protocol id"),
            InvalidVersion => write!(fmt, "invalid version info"),
            PacketTooSmall => write!(fmt, "packet is too small"),
            PayloadAboveLimit => write!(fmt, "payload is above the {} bytes limit", NETCODE_MAX_PAYLOAD_BYTES),
            Expired => write!(fmt, "connection expired"),
            TimedOut => write!(fmt, "connection timed out"),
            DuplicatedSequence => write!(fmt, "sequence already received"),
            Disconnected => write!(fmt, "disconnected"),
            NoMoreServers => write!(fmt, "client has no more servers to connect"),
            CryptoError => write!(fmt, "error while encoding or decoding"),
            NotInHostList => write!(fmt, "token does not contain the server address"),
            ClientNotFound => write!(fmt, "client was not found"),
            ClientNotConnected => write!(fmt, "client is disconnected or connecting"),
            BufferTooSmall => write!(fmt, "buffer"),
            IoError(ref err) => write!(fmt, "{}", err),
            TokenGenerationError(ref err) => write!(fmt, "{}", err),
        }
    }
}

impl error::Error for NetcodeError {}

impl From<io::Error> for NetcodeError {
    fn from(inner: io::Error) -> Self {
        NetcodeError::IoError(inner)
    }
}

impl From<TokenGenerationError> for NetcodeError {
    fn from(inner: TokenGenerationError) -> Self {
        NetcodeError::TokenGenerationError(inner)
    }
}

impl From<CryptoError> for NetcodeError {
    fn from(_: CryptoError) -> Self {
        NetcodeError::CryptoError
    }
}
