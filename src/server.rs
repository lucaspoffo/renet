use crate::channel::ChannelConfig;
use crate::client::{LocalClient, LocalClientConnected};
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::{Packet, Payload, Unauthenticaded};
use crate::protocol::ServerAuthenticationProtocol;
use crate::remote_connection::{ClientId, ConnectionConfig, NetworkInfo, RemoteConnection};

use log::{debug, error, info};

use std::collections::HashMap;
use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, UdpSocket};

pub struct ServerConfig {
    pub max_clients: usize,
    pub max_payload_size: usize,
}

impl ServerConfig {
    pub fn new(max_clients: usize, max_payload_size: usize) -> Self {
        Self {
            max_clients,
            max_payload_size,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_clients: 16,
            max_payload_size: 8 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected(ClientId),
    ClientDisconnected(ClientId),
}

pub struct Server<P: ServerAuthenticationProtocol> {
    config: ServerConfig,
    socket: UdpSocket,
    remote_clients: HashMap<ClientId, RemoteConnection<P>>,
    local_clients: HashMap<ClientId, LocalClient>,
    connecting: HashMap<ClientId, RemoteConnection<P>>,
    channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
    events: VecDeque<ServerEvent>,
    connection_config: ConnectionConfig,
}

impl<P> Server<P>
where
    P: ServerAuthenticationProtocol,
{
    pub fn new(
        socket: UdpSocket,
        config: ServerConfig,
        connection_config: ConnectionConfig,
        channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
    ) -> Result<Self, RenetError> {
        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            remote_clients: HashMap::new(),
            local_clients: HashMap::new(),
            connecting: HashMap::new(),
            config,
            channels_config,
            connection_config,
            events: VecDeque::new(),
        })
    }

    pub fn has_clients(&self) -> bool {
        !self.remote_clients.is_empty() || !self.local_clients.is_empty()
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    fn find_client_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut RemoteConnection<P>> {
        self.remote_clients
            .values_mut()
            .find(|c| *c.addr() == *addr)
    }

    fn find_connecting_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut RemoteConnection<P>> {
        self.connecting.values_mut().find(|c| c.addr() == addr)
    }

    pub fn network_info(&mut self, client_id: ClientId) -> Option<&NetworkInfo> {
        if let Some(connection) = self.remote_clients.get_mut(&client_id) {
            return Some(connection.network_info());
        }
        None
    }

    pub fn create_local_client(&mut self, client_id: u64) -> LocalClientConnected {
        let channels = self.channels_config.keys().copied().collect();
        self.events
            .push_back(ServerEvent::ClientConnected(client_id));
        let (local_client_connected, local_client) = LocalClientConnected::new(client_id, channels);
        self.local_clients.insert(client_id, local_client);
        local_client_connected
    }

    pub fn disconnect(&mut self, client_id: ClientId) {
        if let Some(mut remote_client) = self.remote_clients.remove(&client_id) {
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id));
            if let Err(e) = remote_client
                .send_disconnect_packet(&self.socket, DisconnectionReason::DisconnectedByServer)
            {
                error!(
                    "Failed to send disconnect packet to Client {}: {}",
                    client_id, e
                );
            }
        } else if let Some(mut local_client) = self.local_clients.remove(&client_id) {
            local_client.disconnect();
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id));
        }
    }

    pub fn send_message<C: Into<u8>>(
        &mut self,
        client_id: ClientId,
        channel_id: C,
        message: Vec<u8>,
    ) -> Result<(), RenetError> {
        let channel_id = channel_id.into();
        if let Some(remote_connection) = self.remote_clients.get_mut(&client_id) {
            remote_connection.send_message(channel_id, message)
        } else if let Some(local_connection) = self.local_clients.get_mut(&client_id) {
            local_connection.send_message(channel_id, message)
        } else {
            Err(RenetError::ClientNotFound)
        }
    }

    pub fn broadcast_message<C: Into<u8>>(&mut self, channel_id: C, message: Payload) {
        let channel_id = channel_id.into();
        for remote_client in self.remote_clients.values_mut() {
            if let Err(e) = remote_client.send_message(channel_id, message.clone()) {
                error!("{}", e);
            }
        }

        for local_client in self.local_clients.values_mut() {
            if let Err(e) = local_client.send_message(channel_id, message.clone()) {
                error!("{}", e);
            }
        }
    }

    pub fn broadcast_message_remote<C: Into<u8>>(&mut self, channel_id: C, message: Payload) {
        let channel_id = channel_id.into();
        for remote_client in self.remote_clients.values_mut() {
            if let Err(e) = remote_client.send_message(channel_id, message.clone()) {
                error!("{}", e);
            }
        }
    }

    pub fn broadcast_message_local<C: Into<u8>>(&mut self, channel_id: C, message: Payload) {
        let channel_id = channel_id.into();
        for local_client in self.local_clients.values_mut() {
            if let Err(e) = local_client.send_message(channel_id, message.clone()) {
                error!("{}", e);
            }
        }
    }

    pub fn receive_message<C: Into<u8>>(
        &mut self,
        client_id: ClientId,
        channel_id: C,
    ) -> Result<Option<Payload>, RenetError> {
        let channel_id = channel_id.into();
        if let Some(remote_client) = self.remote_clients.get_mut(&client_id) {
            remote_client.receive_message(channel_id)
        } else if let Some(local_client) = self.local_clients.get_mut(&client_id) {
            local_client.receive_message(channel_id)
        } else {
            Err(RenetError::ClientNotFound)
        }
    }

    pub fn get_clients_id(&self) -> Vec<ClientId> {
        self.remote_clients
            .keys()
            .copied()
            .chain(self.local_clients.keys().copied())
            .collect()
    }

    pub fn is_client_connected(&self, client_id: &ClientId) -> bool {
        self.remote_clients.contains_key(client_id) || self.local_clients.contains_key(client_id)
    }

    pub fn update(&mut self) -> Result<(), RenetError> {
        let mut buffer = vec![0u8; self.config.max_payload_size];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if let Err(e) = self.process_payload_from(&buffer[..len], &addr) {
                        error!("{}", e);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(RenetError::IOError(e)),
            };
        }

        let mut disconnected_remote_clients: Vec<ClientId> = vec![];
        for (&client_id, connection) in self.remote_clients.iter_mut() {
            connection.update();
            if connection.is_disconnected() {
                disconnected_remote_clients.push(client_id);
            }
        }

        for &client_id in disconnected_remote_clients.iter() {
            self.remote_clients.remove(&client_id);
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id));
            info!("Remote Client {} disconnected.", client_id);
        }

        let mut disconnected_local_clients: Vec<ClientId> = vec![];
        for (&client_id, connection) in self.local_clients.iter() {
            if !connection.is_connected() {
                disconnected_local_clients.push(client_id);
            }
        }

        for &client_id in disconnected_local_clients.iter() {
            self.local_clients.remove(&client_id);
            self.events
                .push_back(ServerEvent::ClientDisconnected(client_id));
            info!("Local Client {} disconnected.", client_id);
        }

        self.update_pending_connections();
        Ok(())
    }

    fn update_pending_connections(&mut self) {
        let mut connected_clients = vec![];
        let mut disconnected_clients = vec![];
        for connection in self.connecting.values_mut() {
            connection.update();
            if connection.is_disconnected() {
                disconnected_clients.push(connection.client_id());
                continue;
            }

            if connection.is_connected() {
                connected_clients.push(connection.client_id());
            } else if let Ok(Some(payload)) = connection.create_protocol_payload() {
                let packet = Packet::Unauthenticaded(Unauthenticaded::Protocol { payload });
                send_packet(&self.socket, packet, connection.addr());
            }
        }

        for client_id in disconnected_clients {
            self.connecting
                .remove(&client_id)
                .expect("Disconnected Clients always exists");
            info!("Request connection {} failed.", client_id);
        }

        for client_id in connected_clients {
            let mut connection = self
                .connecting
                .remove(&client_id)
                .expect("Connected Client always exist");
            if self.remote_clients.len() >= self.config.max_clients {
                info!(
                    "Connection from {} successfuly stablished but server was full.",
                    connection.addr()
                );
                let packet = Unauthenticaded::ConnectionError(DisconnectionReason::MaxPlayer);
                send_packet(&self.socket, packet, connection.addr());

                continue;
            }

            info!(
                "Connection stablished with client {} ({}).",
                connection.client_id(),
                connection.addr(),
            );

            for (channel_id, channel_config) in self.channels_config.iter() {
                let channel = channel_config.new_channel();
                connection.add_channel(*channel_id, channel);
            }

            self.events
                .push_back(ServerEvent::ClientConnected(connection.client_id()));
            self.remote_clients
                .insert(connection.client_id(), connection);
        }
    }

    pub fn send_packets(&mut self) {
        for (client_id, connection) in self.remote_clients.iter_mut() {
            if let Err(e) = connection.send_packets(&self.socket) {
                error!("Failed to send packet for Client {}: {:?}", client_id, e);
            }
        }
    }

    fn process_payload_from(
        &mut self,
        payload: &[u8],
        addr: &SocketAddr,
    ) -> Result<(), RenetError> {
        if let Some(client) = self.find_client_by_addr(addr) {
            return client.process_payload(payload);
        }

        if self.remote_clients.len() >= self.config.max_clients {
            let packet = Unauthenticaded::ConnectionError(DisconnectionReason::MaxPlayer);
            try_send_packet(&self.socket, packet, addr)?;
            debug!("Connection Denied to addr {}, server is full.", addr);
            return Ok(());
        }

        match self.find_connecting_by_addr(addr) {
            Some(connection) => {
                return connection.process_payload(payload);
            }
            None => {
                let packet = bincode::deserialize::<Packet>(payload)?;
                if let Packet::Unauthenticaded(Unauthenticaded::Protocol { payload }) = packet {
                    let protocol = P::from_payload(&payload)?;
                    let id = protocol.id();
                    if self.is_client_connected(&id) {
                        info!(
                            "Client with id {} already connected, discarded connection attempt.",
                            id
                        );
                        let packet = Unauthenticaded::ConnectionError(
                            DisconnectionReason::ClientIdAlreadyConnected,
                        );
                        send_packet(&self.socket, packet, addr);
                    } else {
                        info!("Created new protocol from payload with client id {}", id);
                        let new_connection = RemoteConnection::new(
                            protocol.id(),
                            *addr,
                            self.connection_config.clone(),
                            protocol,
                        );
                        self.connecting.insert(id, new_connection);
                    }
                }
            }
        };

        Ok(())
    }
}

fn try_send_packet(
    socket: &UdpSocket,
    packet: impl Into<Packet>,
    addr: &SocketAddr,
) -> Result<(), RenetError> {
    let packet: Packet = packet.into();
    let packet = bincode::serialize(&packet)?;
    socket.send_to(&packet, addr)?;
    Ok(())
}

fn send_packet(socket: &UdpSocket, packet: impl Into<Packet>, addr: &SocketAddr) {
    let packet: Packet = packet.into();
    let packet = match bincode::serialize(&packet) {
        Err(e) => {
            error!("Failed to serialize packet {}", e);
            return;
        }
        Ok(p) => p,
    };
    if let Err(e) = socket.send_to(&packet, addr) {
        error!("Failed to serialize packet {}", e);
    }
}
