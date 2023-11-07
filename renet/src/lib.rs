mod channel;
mod connection_stats;
mod error;
mod packet;
mod remote_connection;
mod server;

#[cfg(feature = "transport")]
pub mod transport;

pub use channel::{ChannelConfig, DefaultChannel, SendType};
pub use error::{ChannelError, ClientNotFound, DisconnectReason};
pub use remote_connection::{ConnectionConfig, RenetConnectionStatus, NetworkInfo, RenetClient};
pub use server::{RenetServer, ServerEvent};

pub use bytes::Bytes;

/// Unique identifier for clients.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Ord, PartialOrd)]
pub struct ClientId(u64);

impl ClientId {
    /// Creates a [`ClientId`] from a raw 64 bit value.
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw 64 bit value of the [`ClientId`]
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ClientId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ClientId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        u64::deserialize(deserializer).map(ClientId::from_raw)
    }
}
