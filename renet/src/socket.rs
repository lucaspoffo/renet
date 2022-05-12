use std::net::{SocketAddr, UdpSocket};

use std::io;

use link_conditioner::{Conditioner, ConditionerConfig, SocketLike};

#[derive(Debug)]
pub enum RawSocket {
    Udp(UdpSocket),
}

impl SocketLike for RawSocket {
    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        match self {
            RawSocket::Udp(socket) => socket.set_nonblocking(nonblocking),
        }
    }
    fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            RawSocket::Udp(socket) => socket.recv(buf),
        }
    }
    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        match self {
            RawSocket::Udp(socket) => socket.recv_from(buf),
        }
    }
    fn send(&self, buf: &[u8]) -> io::Result<usize> {
        match self {
            RawSocket::Udp(socket) => socket.send(buf),
        }
    }
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        match self {
            RawSocket::Udp(socket) => socket.send_to(buf, addr),
        }
    }
}

impl From<UdpSocket> for RawSocket {
    fn from(socket: UdpSocket) -> RawSocket {
        RawSocket::Udp(socket)
    }
}

impl From<UdpSocket> for Socket {
    fn from(socket: UdpSocket) -> Socket {
        Socket::from_socket(socket.into())
    }
}

impl From<Conditioner<RawSocket>> for Socket {
    fn from(socket: Conditioner<RawSocket>) -> Socket {
        Socket::from_conditioner(socket.into())
    }
}

#[derive(Debug)]
pub struct Socket {
    // Yeah this is kind of hacky, but Rust doesn't really like mutating enums in place.
    //
    // Seems like it is either this or using `unsafe`...
    conditioner: Option<Conditioner<RawSocket>>,
    socket: Option<RawSocket>,
}

impl Socket {
    pub fn from_socket(socket: RawSocket) -> Self {
        Self {
            conditioner: None,
            socket: Some(socket),
        }
    }

    pub fn from_conditioner(conditioner: Conditioner<RawSocket>) -> Self {
        Self {
            conditioner: Some(conditioner),
            socket: None,
        }
    }

    pub fn set_conditions(&mut self, config: Option<ConditionerConfig>) {
        match config {
            Some(config) => {
                if let Some(raw) = self.socket.take() {
                    self.conditioner = Some(Conditioner::new(config, raw));
                }
            }
            None => {
                if let Some(conditioner) = self.conditioner.take() {
                    self.socket = Some(conditioner.into_socket());
                }
            }
        }
    }

    pub fn is_conditioned(&mut self) -> bool {
        self.conditioner.is_some()
    }

    pub fn is_raw(&mut self) -> bool {
        self.socket.is_some()
    }
}

impl SocketLike for Socket {
    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        match (self.conditioner.as_ref(), self.socket.as_ref()) {
            (Some(socket), None) => socket.set_nonblocking(nonblocking),
            (None, Some(socket)) => socket.set_nonblocking(nonblocking),
            _ => unreachable!(),
        }
    }
    fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        match (self.conditioner.as_ref(), self.socket.as_ref()) {
            (Some(socket), None) => socket.recv(buf),
            (None, Some(socket)) => socket.recv(buf),
            _ => unreachable!(),
        }
    }
    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        match (self.conditioner.as_ref(), self.socket.as_ref()) {
            (Some(socket), None) => socket.recv_from(buf),
            (None, Some(socket)) => socket.recv_from(buf),
            _ => unreachable!(),
        }
    }
    fn send(&self, buf: &[u8]) -> io::Result<usize> {
        match (self.conditioner.as_ref(), self.socket.as_ref()) {
            (Some(socket), None) => socket.send(buf),
            (None, Some(socket)) => socket.send(buf),
            _ => unreachable!(),
        }
    }
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        match (self.conditioner.as_ref(), self.socket.as_ref()) {
            (Some(socket), None) => socket.send_to(buf, addr),
            (None, Some(socket)) => socket.send_to(buf, addr),
            _ => unreachable!(),
        }
    }
}
