use crate::connection_control::ConnectionControl;
use crate::error::DisconnectionReason;
use crate::packet::{Authenticated, Message, Packet, Payload, Unauthenticaded};
use crate::protocol::{SecurityService, ServerAuthenticationProtocol};
use crate::transport::TransportServer;
use crate::ClientId;

use log::{debug, error};

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::error::Error;

struct Connecting<P> {
    addr: SocketAddr,
    disconnect_reason: Option<DisconnectionReason>,
    protocol: P,
}

struct Connected<S> {
    addr: SocketAddr,
    service: S,
}

pub struct UdpServer<C, P: ServerAuthenticationProtocol<C>> {
    socket: UdpSocket,
    connecting_clients: HashMap<C, Connecting<P>>,
    connected_clients: HashMap<C, Connected<P::Service>>,
}

impl<C: ClientId, P: ServerAuthenticationProtocol<C>> UdpServer<C, P> {
    pub fn new(socket: UdpSocket) -> Self {
        socket.set_nonblocking(true).unwrap();

        Self {
            socket,
            connecting_clients: HashMap::new(),
            connected_clients: HashMap::new(),
        }
    }

    fn find_connecting_by_addr(&mut self, addr: SocketAddr) -> Option<(&C, &mut Connecting<P>)> {
        self.connecting_clients
            .iter_mut()
            .find(|(_, c)| c.addr == addr)
    }

    fn find_connected_by_addr(
        &mut self,
        addr: SocketAddr,
    ) -> Option<(&C, &mut Connected<P::Service>)> {
        self.connected_clients
            .iter_mut()
            .find(|(_, c)| c.addr == addr)
    }
}


impl<C: ClientId, P: ServerAuthenticationProtocol<C>> TransportServer for UdpServer<C, P> {
    type ClientId = C;
    type ConnectionId = SocketAddr;

    fn connection_id(&self) -> Self::ConnectionId {
        self.socket.local_addr().unwrap()
    }

    fn recv(
        &mut self,
        connection_control: &ConnectionControl<Self::ClientId>,
    ) -> Result<Option<(Self::ClientId, Payload)>, Box<dyn Error + Send + Sync + 'static>> {
        let mut buffer = vec![0u8; 1200];

        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    // debug!("Received packet from addr: {}", addr);
                    if let Ok(packet) = bincode::deserialize::<Packet>(&buffer[..len]) {
                        if let Some((client_id, connecting)) = self.find_connecting_by_addr(addr) {
                            match packet {
                                Packet::Authenticated(_) => {
                                    debug!("Dropped AuthenticatedPacket, client is connecting.")
                                }
                                Packet::Unauthenticaded(Unauthenticaded::ConnectionError(
                                    error,
                                )) => {
                                    error!("Client {} disconnected: {}", client_id, error);
                                    connecting.disconnect_reason = Some(error)
                                }
                                Packet::Unauthenticaded(Unauthenticaded::Protocol {
                                    ref payload,
                                }) => {
                                    debug!("Received protocol packet");
                                    if let Err(e) = connecting.protocol.read_payload(payload) {
                                        error!("Error reading protocol payload: {}", e);
                                    }
                                }
                            }
                        } else if let Some((client_id, connected)) =
                            self.find_connected_by_addr(addr)
                        {
                            match packet {
                                Packet::Unauthenticaded(_) => {
                                    debug!("Dropped UnauthenticatedPacket, client is connected.")
                                }
                                Packet::Authenticated(Authenticated { ref payload }) => {
                                    match connected.service.ss_unwrap(payload) {
                                        Err(e) => error!("Error unwrapping packet: {}", e),
                                        Ok(payload) => return Ok(Some((*client_id, payload))),
                                    }
                                }
                            }
                        } else if let Packet::Unauthenticaded(Unauthenticaded::Protocol {
                            ref payload,
                        }) = packet
                        {
                            if let Ok(protocol) = P::from_payload(payload) {
                                if connection_control.is_client_permitted(protocol.id()) {
                                    let connecting = Connecting {
                                        protocol,
                                        addr,
                                        disconnect_reason: None,
                                    };
                                    self.connecting_clients
                                        .insert(connecting.protocol.id(), connecting);
                                } else {
                                    let packet =
                                        Packet::Unauthenticaded(Unauthenticaded::ConnectionError(
                                            DisconnectionReason::Denied,
                                        ));
                                    if let Ok(packet) = bincode::serialize(&packet) {
                                        if let Err(e) = self.socket.send_to(&packet, addr) {
                                            error!("Error sending disconnect packet: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(Box::new(e)),
            };
        }
    }

    fn send(
        &mut self,
        client_id: Self::ClientId,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        if let Some(connected) = self.connected_clients.get_mut(&client_id) {
            let payload = connected.service.ss_wrap(payload).unwrap();
            let packet = Packet::Authenticated(Authenticated { payload });
            let payload = bincode::serialize(&packet).map_err(Box::new)?;
            self.socket
                .send_to(&payload, connected.addr)
                .map_err(Box::new)?;
        }
        Ok(())
    }

    fn update(&mut self) -> Vec<Self::ClientId> {
        let mut new_connections = vec![];
        let mut disconnected = vec![];

        for (client_id, connecting) in self.connecting_clients.iter_mut() {
            if connecting.protocol.is_authenticated() {
                debug!("Client {} has been authenticated.", client_id);
                new_connections.push(*client_id);
            } else {
                match connecting.protocol.create_payload() {
                    Err(e) => {
                        error!("Protocol error: {}", e);
                        disconnected.push(client_id);
                    }
                    Ok(Some(payload)) => {
                        let packet = Packet::Unauthenticaded(Unauthenticaded::Protocol { payload });
                        let payload = bincode::serialize(&packet).unwrap();

                        if let Err(e) = self.socket.send_to(&payload, connecting.addr) {
                            error!("Error sending protocol packet: {}", e);
                        }
                    }
                    Ok(None) => {}
                }
            }
        }

        new_connections
    }

    fn disconnect(&mut self, client_id: &Self::ClientId, reason: DisconnectionReason) {
        if let Some(mut client) = self.connected_clients.remove(client_id) {
            let message = Message::Disconnect(reason);
            if let Ok(payload) = bincode::serialize(&message) {
                if let Ok(payload) = client.service.ss_wrap(&payload) {
                    let packet = Packet::Authenticated(Authenticated { payload });
                    if let Ok(packet) = bincode::serialize(&packet) {
                        if let Err(e) = self.socket.send_to(&packet, &client.addr) {
                            error!("Error sending disconnect packet: {}", e);
                        }
                    }
                }
            }
        }

        if let Some(client) = self.connecting_clients.remove(&client_id) {
            let packet = Packet::Unauthenticaded(Unauthenticaded::ConnectionError(reason));
            if let Ok(packet) = bincode::serialize(&packet) {
                if let Err(e) = self.socket.send_to(&packet, &client.addr) {
                    error!("Error sending disconnect packet: {}", e);
                }
            }
        }
    }

    fn is_authenticated(&self, client_id: Self::ClientId) -> bool {
        if self.connected_clients.contains_key(&client_id) {
            return true;
        }

        if let Some(connecting) = self.connecting_clients.get(&client_id) {
            return connecting.protocol.is_authenticated();
        }

        false
    }

    fn confirm_connect(&mut self, client_id: Self::ClientId) {
        if let Some(connecting) = self.connecting_clients.remove(&client_id) {
            let service = connecting.protocol.build_security_interface();
            let connected: Connected<P::Service> = Connected {
                addr: connecting.addr,
                service,
            };
            self.connected_clients.insert(client_id, connected);
            debug!("Confirmed Client {} connection", client_id);
        }
    }
}
