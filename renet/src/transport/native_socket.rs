use std::net::{SocketAddr, UdpSocket};

use super::{NetcodeError, NetcodeTransportError, TransportSocket};

/// Implementation of [`TransportSocket`] for `UdpSockets`.
#[derive(Debug)]
pub struct NativeSocket {
    socket: UdpSocket,
}

impl NativeSocket {
    /// Makes a new native socket.
    pub fn new(socket: UdpSocket) -> Result<Self, NetcodeError> {
        socket.set_nonblocking(true)?;
        Ok(Self{ socket })
    }
}

impl TransportSocket for NativeSocket {
    fn addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    fn is_closed(&mut self) -> bool {
        false
    }

    fn close(&mut self) { }
    fn disconnect(&mut self, _: SocketAddr) { }
    fn preupdate(&mut self) { }

    fn try_recv(&mut self, buffer: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buffer)
    }

    fn postupdate(&mut self) { }

    fn send(&mut self, addr: SocketAddr, packet: &[u8]) -> Result<(), NetcodeTransportError> {
        self.socket.send_to(packet, addr)?;
        Ok(())
    }
}
