use crate::packet::Payload;
use crossbeam_channel::{unbounded, Receiver};
use log::error;
use std::{
    error::Error,
    net::{SocketAddr, UdpSocket},
};
use steamworks::{CallbackHandle, Client, ClientManager, P2PSessionRequest, SendType, SteamId};

pub trait Transport {
    type ConnectionId;
    fn recv(
        &mut self,
    ) -> Result<Option<(Self::ConnectionId, Payload)>, Box<dyn Error + Send + Sync + 'static>>;
    fn send(
        &mut self,
        connection_id: Self::ConnectionId,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;
    // fn close(&mut self, connection_id: ConnectionId);
    fn connection_request(&mut self) -> Option<Self::ConnectionId>;
    fn allow_connection(&mut self, connection_id: Self::ConnectionId);
    fn close_connection(&mut self, connection_id: Self::ConnectionId);
}

struct UdpTransport {
    socket: UdpSocket,
    buffer: Vec<u8>,
}

impl UdpTransport {
    pub fn new(socket: UdpSocket, buffer_size: usize) -> Result<Self, std::io::Error> {
        socket.set_nonblocking(true)?;
        let buffer = vec![0; buffer_size];

        Ok(Self { socket, buffer })
    }
}

impl Transport for UdpTransport {
    type ConnectionId = SocketAddr;
    fn recv(
        &mut self,
    ) -> Result<Option<(SocketAddr, Payload)>, Box<dyn Error + Send + Sync + 'static>> {
        match self.socket.recv_from(&mut self.buffer) {
            Ok((len, addr)) => {
                let payload = self.buffer[..len].to_vec();
                Ok(Some((addr, payload)))
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(Box::new(e)),
        }
    }

    fn send(
        &mut self,
        connection_id: SocketAddr,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        self.socket
            .send_to(payload, connection_id)
            .map_err(|e| Box::new(e))?;
        Ok(())
    }

    fn connection_request(&mut self) -> Option<SocketAddr> {
        None
    }

    fn allow_connection(&mut self, _connection_id: SocketAddr) {}

    fn close_connection(&mut self, _connection_id: SocketAddr) {}
}

struct SteamTransport {
    client: Client<ClientManager>,
    _session_request: CallbackHandle<ClientManager>,
    receiver: Receiver<SteamId>,
}

impl SteamTransport {
    pub fn bind(client: Client<ClientManager>) -> Self {
        let (sender, receiver) = unbounded();

        Self {
            _session_request: client.register_callback(move |request: P2PSessionRequest| {
                if let Err(e) = sender.send(request.remote) {
                    error!("Error sending new connection request: {}", e);
                }
            }),
            receiver,
            client,
        }
    }
}

impl Transport for SteamTransport {
    type ConnectionId = SteamId;

    fn recv(
        &mut self,
    ) -> Result<Option<(SteamId, Payload)>, Box<dyn Error + Send + Sync + 'static>> {
        if let Some(size) = self.client.networking().is_p2p_packet_available() {
            let mut read_buf = vec![0u8; size];
            let (remote, _) = self
                .client
                .networking()
                .read_p2p_packet(read_buf.as_mut())
                .unwrap();

            return Ok(Some((remote, read_buf)));
        }
        Ok(None)
    }

    fn send(
        &mut self,
        connection_id: SteamId,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        if !self
            .client
            .networking()
            .send_p2p_packet(connection_id, SendType::Unreliable, payload)
        {
            error!("Error while sending message to {:?}", connection_id);
        }
        Ok(())
    }

    fn connection_request(&mut self) -> Option<SteamId> {
        self.receiver.recv().ok()
    }

    fn allow_connection(&mut self, connection_id: SteamId) {
        self.client.networking().accept_p2p_session(connection_id);
    }

    fn close_connection(&mut self, connection_id: SteamId) {
        self.client.networking().close_p2p_session(connection_id);
    }
}
