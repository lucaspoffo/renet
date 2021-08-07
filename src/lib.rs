pub mod channel;
pub mod client;
pub mod error;
mod packet;
pub mod protocol;
mod reassembly_fragment;
pub mod remote_connection;
mod sequence_buffer;
pub mod server;
mod timer;
// pub mod transport;

use crate::{
    error::DisconnectionReason,
    packet::{Authenticated, Packet, Unauthenticaded},
};
use packet::{Message, Payload};
use protocol::{AuthenticationProtocol, ServerAuthenticationProtocol};
use server::ConnectionPermission;
pub(crate) use timer::Timer;

use log::{debug, error};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt::{Debug, Display},
    hash::Hash,
    net::{SocketAddr, UdpSocket},
};

use crate::protocol::SecurityService;
use thiserror::Error;

pub trait ClientId: Display + Clone + Copy + Debug + Hash + Eq {}
impl<T> ClientId for T where T: Display + Copy + Debug + Hash + Eq {}

pub struct ConnectionControl<C> {
    allow_clients: HashSet<C>,
    deny_clients: HashSet<C>,
    connection_permission: ConnectionPermission,
}

impl<C: ClientId> ConnectionControl<C> {
    pub fn new(connection_permission: ConnectionPermission) -> Self {
        Self {
            connection_permission,
            allow_clients: HashSet::new(),
            deny_clients: HashSet::new(),
        }
    }

    pub fn allow_client(&mut self, client_id: &C) {
        self.allow_clients.insert(*client_id);
        self.deny_clients.remove(client_id);
    }

    pub fn deny_client(&mut self, client_id: &C) {
        self.deny_clients.insert(*client_id);
        self.allow_clients.remove(client_id);
    }

    pub fn set_connection_permission(&mut self, connection_permission: ConnectionPermission) {
        self.connection_permission = connection_permission;
    }

    pub fn connection_permission(&self) -> &ConnectionPermission {
        &self.connection_permission
    }

    pub fn is_client_permitted(&self, client_id: C) -> bool {
        if self.deny_clients.contains(&client_id) {
            return false;
        }

        match self.connection_permission {
            ConnectionPermission::All => true,
            ConnectionPermission::OnlyAllowed => self.allow_clients.contains(&client_id),
            ConnectionPermission::None => false,
        }
    }

    pub fn allowed_clients(&self) -> Vec<C> {
        self.allow_clients.iter().copied().collect()
    }

    pub fn denied_clients(&self) -> Vec<C> {
        self.deny_clients.iter().copied().collect()
    }
}

pub trait TransportClient {
    fn recv(&mut self) -> Option<Payload>;
    fn update(&mut self);
    fn connection_error(&self) -> Option<&(dyn std::error::Error + 'static)>;
    fn send_to_server(&mut self, payload: &[u8]);
    fn disconnect(&mut self, reason: DisconnectionReason);
    fn is_authenticated(&self) -> bool;
}

enum ClientState<C, P: AuthenticationProtocol<C>> {
    Connecting { protocol: P },
    Connected { security_service: P::Service },
    Disconnected,
}

struct UdpClient<C, P: AuthenticationProtocol<C>> {
    socket: UdpSocket,
    server_addr: SocketAddr,
    state: ClientState<C, P>,
    error: Option<UdpError>,
}

impl<C, P: AuthenticationProtocol<C>> UdpClient<C, P> {
    pub fn new(server_addr: SocketAddr, protocol: P, socket: UdpSocket) -> Self {
        let state = ClientState::Connecting { protocol };
        socket.set_nonblocking(true).unwrap();

        Self {
            socket,
            server_addr,
            state,
            error: None,
        }
    }
}

impl<C, P> TransportClient for UdpClient<C, P>
where
    C: ClientId,
    P: AuthenticationProtocol<C>,
{
    fn recv(&mut self) -> Option<Payload> {
        if self.error.is_some() || matches!(self.state, ClientState::Disconnected) {
            return None;
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
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => return None,
                Err(e) => {
                    self.error = Some(e.into());
                    return None;
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
                                return Some(payload);
                            }
                        }
                    },
                    ClientState::Connecting { ref mut protocol } => match packet {
                        Packet::Unauthenticaded(Unauthenticaded::ConnectionError(_)) => {
                            self.error = Some(UdpError::Disconnected);
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
        if self.error.is_some() {
            self.state = ClientState::Disconnected;
            return;
        }

        if let ClientState::Connecting { ref protocol } = self.state {
            if protocol.is_authenticated() {
                let security_service = protocol.build_security_interface();
                self.state = ClientState::Connected { security_service };
            }
        }
    }

    fn send_to_server(&mut self, payload: &[u8]) {
        match self.state {
            ClientState::Connecting { .. } => {}
            ClientState::Connected {
                ref mut security_service,
            } => {
                if let Ok(payload) = security_service.ss_wrap(payload) {
                    let packet = Packet::Authenticated(Authenticated {
                        payload: payload.to_vec(),
                    });
                    if let Ok(payload) = bincode::serialize(&packet) {
                        if let Err(e) = self.socket.send_to(&payload, &self.server_addr) {
                            error!("Error sending packet to server: {}", e);
                        }
                    }
                }
            }
            ClientState::Disconnected => {}
        }
    }

    fn connection_error(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Some(e) = self.error.as_ref() {
            let error: &(dyn std::error::Error + 'static) = e;
            return Some(error);
        }
        None
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

    fn is_authenticated(&self) -> bool {
        matches!(self.state, ClientState::Connected { .. })
    }
}

struct Connecting<P> {
    addr: SocketAddr,
    error: Option<DisconnectionReason>,
    protocol: P,
}

struct Connected<S> {
    addr: SocketAddr,
    service: S,
}

#[derive(Debug, Error)]
pub enum UdpError {
    #[error("socket disconnected: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Client disconnected")]
    Disconnected,
}

struct UdpServer<C, P: ServerAuthenticationProtocol<C>> {
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

pub trait TransportServer {
    type ClientId;

    fn recv(
        &mut self,
        connection_control: &ConnectionControl<Self::ClientId>,
    ) -> Result<Option<(Self::ClientId, Payload)>, Box<dyn Error + Send + Sync + 'static>>;

    fn send(
        &mut self,
        client_id: Self::ClientId,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;

    fn confirm_connect(&mut self, client_id: Self::ClientId);
    fn disconnect(&mut self, client_id: Self::ClientId, reason: DisconnectionReason);
    fn is_authenticated(&self, client_id: Self::ClientId) -> bool;
    fn update(&mut self) -> Vec<Self::ClientId>;
}

impl<C: ClientId, P: ServerAuthenticationProtocol<C>> TransportServer for UdpServer<C, P> {
    type ClientId = C;

    fn recv(
        &mut self,
        connection_control: &ConnectionControl<Self::ClientId>,
    ) -> Result<Option<(Self::ClientId, Payload)>, Box<dyn Error + Send + Sync + 'static>> {
        let mut buffer = vec![0u8; 1200];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
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
                                    connecting.error = Some(error)
                                }
                                Packet::Unauthenticaded(Unauthenticaded::Protocol {
                                    ref payload,
                                }) => {
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
                                        error: None,
                                    };
                                    self.connecting_clients
                                        .insert(connecting.protocol.id(), connecting);
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
        if let Some(connected) = self.connected_clients.get(&client_id) {
            let packet = Packet::Authenticated(Authenticated {
                payload: payload.to_vec(),
            });
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
                new_connections.push(*client_id);
            } else {
                match connecting.protocol.create_payload() {
                    Err(e) => {
                        error!("Protocol error: {}", e);
                        disconnected.push(client_id);
                    }
                    Ok(Some(payload)) => {
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

    fn disconnect(&mut self, client_id: Self::ClientId, reason: DisconnectionReason) {
        if let Some(mut client) = self.connected_clients.remove(&client_id) {
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
        }
    }
}
