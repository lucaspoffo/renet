use crate::error::{DisconnectionReason, RenetError};
use crate::packet::{Authenticated, Message, Packet, Payload, Unauthenticaded};
use crate::protocol::{AuthenticationProtocol, SecurityService};
use crate::transport::TransportClient;
use crate::ClientId;

use log::{debug, error};
use std::net::{SocketAddr, UdpSocket};


enum ClientState<C, P: AuthenticationProtocol<C>> {
    Connecting { protocol: P },
    Connected { security_service: P::Service },
    Disconnected,
}

pub struct UdpClient<C, P: AuthenticationProtocol<C>> {
    socket: UdpSocket,
    server_addr: SocketAddr,
    state: ClientState<C, P>,
    disconnect_reason: Option<DisconnectionReason>,
}

impl<C, P: AuthenticationProtocol<C>> UdpClient<C, P> {
    pub fn new(server_addr: SocketAddr, protocol: P, socket: UdpSocket) -> Self {
        let state = ClientState::Connecting { protocol };
        socket.set_nonblocking(true).unwrap();

        Self {
            socket,
            server_addr,
            state,
            disconnect_reason: None,
        }
    }
}

impl<C, P> TransportClient for UdpClient<C, P>
where
    C: ClientId,
    P: AuthenticationProtocol<C>,
{
    fn recv(&mut self) -> Result<Option<Payload>, RenetError> {
        if let Some(reason) = self.disconnect_reason {
            return Err(RenetError::ConnectionError(reason));
        }

        loop {
            let mut buffer = vec![0u8; 1200];
            let payload = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        &buffer[..len]
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(_) => {
                    let reason = DisconnectionReason::TransportError;
                    self.disconnect_reason = Some(reason);
                    return Err(RenetError::ConnectionError(reason));
                }
            };

            if let Ok(packet) = bincode::deserialize::<Packet>(&payload) {
                match self.state {
                    ClientState::Disconnected => unreachable!(),
                    ClientState::Connected {
                        ref mut security_service,
                    } => match packet {
                        Packet::Unauthenticaded(_) => {
                            debug!("Dropped unauthenticaded packet, client already connected");
                        }
                        Packet::Authenticated(Authenticated { payload }) => {
                            if let Ok(payload) = security_service.ss_unwrap(&payload) {
                                return Ok(Some(payload));
                            } else {
                                error!("Error security unwrap");
                            }
                        }
                    },
                    ClientState::Connecting { ref mut protocol } => match packet {
                        Packet::Unauthenticaded(Unauthenticaded::ConnectionError(reason)) => {
                            self.disconnect_reason = Some(reason);
                            return Err(RenetError::ConnectionError(reason));
                        }

                        Packet::Unauthenticaded(Unauthenticaded::Protocol { payload }) => {
                            if let Err(e) = protocol.read_payload(&payload) {
                                error!("Failed to read protocol payload: {}", e);
                            }
                        }
                        Packet::Authenticated(Authenticated { .. }) => {
                            debug!("Dropped authenticated packet, client not connected");
                        }
                    },
                }
            }
        }
    }

    fn update(&mut self) {
        if self.disconnect_reason.is_some() {
            self.state = ClientState::Disconnected;
            return;
        }

        if let ClientState::Connecting { ref mut protocol } = self.state {
            if protocol.is_authenticated() {
                let security_service = protocol.build_security_interface();
                self.state = ClientState::Connected { security_service };
            } else if let Ok(Some(payload)) = protocol.create_payload() {
                let packet = Packet::Unauthenticaded(Unauthenticaded::Protocol { payload });
                let payload = bincode::serialize(&packet).unwrap();

                if let Err(e) = self.socket.send_to(&payload, &self.server_addr) {
                    self.disconnect_reason = Some(DisconnectionReason::TransportError);
                    error!("Error sending packet to server: {}", e);
                }
            }
        }
    }

    fn send_to_server(&mut self, payload: &[u8]) {
        match self.state {
            ClientState::Connecting { .. } => {}
            ClientState::Connected {
                ref mut security_service,
            } => {
                let payload = security_service.ss_wrap(payload).unwrap();
                let packet = Packet::Authenticated(Authenticated { payload });
                let payload = bincode::serialize(&packet).unwrap();
                if let Err(e) = self.socket.send_to(&payload, &self.server_addr) {
                    error!("Error sending packet to server: {}", e);
                }
            }
            ClientState::Disconnected => {}
        }
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        self.disconnect_reason
    }

    fn disconnect(&mut self, reason: DisconnectionReason) {
        match self.state {
            ClientState::Disconnected => {}
            ClientState::Connecting { .. } => {
                let packet = Packet::Unauthenticaded(Unauthenticaded::ConnectionError(reason));
                if let Ok(packet) = bincode::serialize(&packet) {
                    if let Err(e) = self.socket.send_to(&packet, &self.server_addr) {
                        error!("Error sending disconnect packet: {}", e);
                    }
                }
            }
            ClientState::Connected {
                ref mut security_service,
            } => {
                let message = Message::Disconnect(reason);
                if let Ok(payload) = bincode::serialize(&message) {
                    if let Ok(payload) = security_service.ss_wrap(&payload) {
                        let packet = Packet::Authenticated(Authenticated { payload });
                        if let Ok(packet) = bincode::serialize(&packet) {
                            if let Err(e) = self.socket.send_to(&packet, &self.server_addr) {
                                error!("Error sending disconnect packet: {}", e);
                            }
                        }
                    }
                }
            }
        }
        self.state = ClientState::Disconnected;
    }

    fn is_connected(&self) -> bool {
        matches!(self.state, ClientState::Connected { .. })
    }
}
