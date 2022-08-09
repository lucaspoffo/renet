use std::{
    error::Error,
    io,
    net::{SocketAddr, UdpSocket},
};

pub trait Transport {
    fn recv_from(&mut self, buffer: &mut [u8]) -> Result<Option<(usize, SocketAddr)>, Box<dyn Error + Send + Sync + 'static>>;
    fn send_to(&mut self, buffer: &[u8], addr: SocketAddr) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;
}

pub struct UdpTransport(UdpSocket);

impl UdpTransport {
    pub fn new() -> Result<Self, io::Error> {
        let addr = SocketAddr::from(([127, 0, 0, 1], 0));
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;

        Ok(Self(socket))
    }

    pub fn with_socket(socket: UdpSocket) -> Result<Self, io::Error> {
        socket.set_nonblocking(true)?;

        Ok(Self(socket))
    }
}

impl Transport for UdpTransport {
    fn send_to(&mut self, buffer: &[u8], addr: SocketAddr) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        self.0.send_to(buffer, addr)?;

        Ok(())
    }

    fn recv_from(&mut self, buffer: &mut [u8]) -> Result<Option<(usize, SocketAddr)>, Box<dyn Error + Send + Sync + 'static>> {
        match self.0.recv_from(buffer) {
            Ok((len, addr)) => Ok(Some((len, addr))),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(Box::new(e)),
        }
    }
}
