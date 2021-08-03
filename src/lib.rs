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

use crate::error::DisconnectionReason;
use packet::Payload;
use protocol::ServerAuthenticationProtocol;
use server::ConnectionPermission;
pub(crate) use timer::Timer;

use log::error;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt::{Debug, Display},
    hash::Hash,
    net::{SocketAddr, UdpSocket},
};

use crate::protocol::SecurityService;

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

struct Connecting<P> {
    addr: SocketAddr,
    protocol: P,
}

struct Connected<S> {
    addr: SocketAddr,
    service: S,
}

struct UdpServer<C, P: ServerAuthenticationProtocol<C>> {
    socket: UdpSocket,
    connecting_clients: HashMap<C, Connecting<P>>,
    connected_clients: HashMap<C, Connected<P::Service>>,
}

impl<C: ClientId, P: ServerAuthenticationProtocol<C>> UdpServer<C, P> {
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

// TODO(transport): Would make sense to divide this trait in Transport and TransportServer
// When impl this for UdpClient, alot of thing will not make sense.
pub trait Transport {
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

    fn disconnect(&mut self, client_id: Self::ClientId, reason: DisconnectionReason);

    fn confirm_connect(&mut self, client_id: Self::ClientId);

    fn update(&mut self) -> Vec<Self::ClientId>;
}

impl<C: ClientId, P: ServerAuthenticationProtocol<C>> Transport for UdpServer<C, P> {
    type ClientId = C;

    fn recv(
        &mut self,
        connection_control: &ConnectionControl<Self::ClientId>,
    ) -> Result<Option<(Self::ClientId, Payload)>, Box<dyn Error + Send + Sync + 'static>> {
        let mut buffer = vec![0u8; 1200];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if let Some((_, connecting)) = self.find_connecting_by_addr(addr) {
                        if let Err(e) = connecting.protocol.read_payload(&buffer[..len]) {
                            error!("Error reading protocol payload: {}", e);
                        }
                    } else if let Some((client_id, connected)) = self.find_connected_by_addr(addr) {
                        match connected.service.ss_unwrap(&buffer[..len]) {
                            Err(e) => error!("Error unwrapping packet: {}", e),
                            Ok(payload) => return Ok(Some((*client_id, payload))),
                        }
                    } else {
                        if let Ok(protocol) = P::from_payload(&buffer[..len]) {
                            if connection_control.is_client_permitted(protocol.id()) {
                                let connecting = Connecting { protocol, addr };
                                self.connecting_clients
                                    .insert(connecting.protocol.id(), connecting);
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
            self.socket
                .send_to(payload, connected.addr)
                .map_err(|e| Box::new(e))?;
        }
        Ok(())
    }

    fn disconnect(&mut self, client_id: C, reason: DisconnectionReason) {
        todo!();
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
