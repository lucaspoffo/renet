use std::fmt::Debug;
use std::net::SocketAddr;

use super::NetcodeTransportError;

/// Unreliable data source for use in [`NetcodeServerTransport`] and [`NetcodeClientTransport`].
///
/// Note that while `netcode` uses `SocketAddr` everywhere, if your transport uses a different 'connection URL'
/// scheme then you can layer the bytes into the [`ConnectToken`](renet::ConnectToken) server address list.
/// Just keep in mind that when a client disconnects, the client will traverse the server address list to find
/// an address to reconnect with. If that isn't supported by your scheme, then when [`TransportSocket::send`] is
/// called with an invalid/unexpected server address you should return an error. If you want to support
/// multiple server addresses but your URLs exceed 16 bytes (IPV6 addresses are 16 bytes), then you should pre-parse
/// the server list from the connect token, and then map that list to the 16-byte IPV6 segments that will be produced
/// by the client when it tries to reconnect to different servers.
pub trait TransportSocket: Debug + Send + Sync + 'static {
    /// Gets the encryption behavior of the socket.
    ///
    /// If the socket internally encrypts packets before sending them, then this should return `true`.
    /// In that case, `renetcode` will not pre-encrypt packets before [`Self::send`] is called.
    fn is_encrypted(&self) -> bool;

    /// Gets the data source's `SocketAddr`.
    fn addr(&self) -> std::io::Result<SocketAddr>;

    /// Checks if the data source is closed.
    fn is_closed(&mut self) -> bool;

    /// Closes the data source.
    ///
    /// This should disconnect any remote connections that are being tracked.
    fn close(&mut self);

    /// Disconnects a remote connection with the given address.
    fn disconnect(&mut self, addr: SocketAddr);

    /// Handles data-source-specific logic that must run before receiving packets.
    fn preupdate(&mut self);

    /// Tries to receive the next packet sent to this data source.
    ///
    /// Returns the number of bytes written to the buffer, and the source address of the bytes.
    fn try_recv(&mut self, buffer: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;

    /// Handles data-source-specific logic that must run after sending packets.
    fn postupdate(&mut self);

    /// Sends a packet to the designated address.
    fn send(&mut self, addr: SocketAddr, packet: &[u8]) -> Result<(), NetcodeTransportError>;
}
