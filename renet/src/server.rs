use crate::{
    network_info::{ClientPacketInfo, NetworkInfo, PacketInfo},
    RenetConnectionConfig,
};

use std::{
    collections::{HashMap, VecDeque},
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use log::error;
use rechannel::{disconnect_packet, error::DisconnectionReason, server::RechannelServer, Bytes};
use renetcode::{NetcodeServer, ServerResult, NETCODE_KEY_BYTES, NETCODE_USER_DATA_BYTES};

/// A server that can establish authenticated connections with multiple clients.
/// Can send/receive encrypted messages from/to them.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct RenetServer {
    socket: UdpSocket,
    reliable_server: RechannelServer<u64>,
    netcode_server: NetcodeServer,
    bandwidth_smoothing_factor: f32,
    clients_packet_info: HashMap<SocketAddr, ClientPacketInfo>,
    buffer: Box<[u8]>,
    events: VecDeque<ServerEvent>,
}

/// Events that can occur in the server.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected(u64, Box<[u8; NETCODE_USER_DATA_BYTES]>),
    ClientDisconnected(u64),
}

/// Configuration to establish a secure or unsecure connection with the server.
pub enum ServerAuthentication {
    /// Establishes a safe connection using a private key for encryption. The private key cannot be
    /// shared with the client. Connections are stablished using
    /// [ConnectTokens][crate::ConnectToken].
    ///
    /// See also [ClientAuthentication::Secure][crate::ClientAuthentication::Secure]
    Secure { private_key: [u8; NETCODE_KEY_BYTES] },
    /// Establishes unsafe connections with clients, useful for testing and prototyping.
    ///
    /// See also [ClientAuthentication::Unsecure][crate::ClientAuthentication::Unsecure]
    Unsecure,
}

/// Configuration options for the renet server.
pub struct ServerConfig {
    /// Maximum numbers of clients that can be connected at a time
    pub max_clients: usize,
    /// Unique identifier to this game/application
    /// One could use a hash function with the game current version to generate this value.
    /// So old version would be unable to connect to newer versions.
    pub protocol_id: u64,
    /// Publicly available address that clients will try to connect to. This is
    /// the address used to generate the ConnectToken when using the secure authentication.
    pub public_addr: SocketAddr,
    /// Authentication configuration for the server
    pub authentication: ServerAuthentication,
}

impl ServerConfig {
    pub fn new(max_clients: usize, protocol_id: u64, public_addr: SocketAddr, authentication: ServerAuthentication) -> Self {
        Self {
            max_clients,
            protocol_id,
            public_addr,
            authentication,
        }
    }
}

impl RenetServer {
    pub fn new(
        current_time: Duration,
        server_config: ServerConfig,
        connection_config: RenetConnectionConfig,
        socket: UdpSocket,
    ) -> Result<Self, std::io::Error> {
        let buffer = vec![0u8; connection_config.max_packet_size as usize].into_boxed_slice();
        let bandwidth_smoothing_factor = connection_config.bandwidth_smoothing_factor;
        let reliable_server = RechannelServer::new(current_time, connection_config.to_connection_config());

        // For unsecure connections we use an fixed private key.
        let private_key = match server_config.authentication {
            ServerAuthentication::Unsecure => [0; NETCODE_KEY_BYTES],
            ServerAuthentication::Secure { private_key } => private_key,
        };

        let netcode_server = NetcodeServer::new(
            current_time,
            server_config.max_clients,
            server_config.protocol_id,
            server_config.public_addr,
            private_key,
        );

        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            netcode_server,
            reliable_server,
            bandwidth_smoothing_factor,
            buffer,
            clients_packet_info: HashMap::new(),
            events: VecDeque::new(),
        })
    }

    #[doc(hidden)]
    pub fn __test() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let server_config = ServerConfig::new(64, 0, socket.local_addr().unwrap(), ServerAuthentication::Unsecure);
        Self::new(Duration::ZERO, server_config, RenetConnectionConfig::default(), socket).unwrap()
    }

    pub fn addr(&self) -> SocketAddr {
        self.netcode_server.address()
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    /// Disconnects a client.
    pub fn disconnect(&mut self, client_id: u64) {
        let current_time = self.netcode_server.current_time();
        let server_result = self.netcode_server.disconnect(client_id);
        if let Err(e) = handle_server_result(
            server_result,
            current_time,
            self.bandwidth_smoothing_factor,
            &self.socket,
            &mut self.reliable_server,
            &mut self.clients_packet_info,
            &mut self.events,
        ) {
            error!("Failed to send disconnect packet to client {}: {}", client_id, e);
        }
    }

    /// Disconnects all connected clients.
    pub fn disconnect_clients(&mut self) {
        for client_id in self.netcode_server.clients_id() {
            self.disconnect(client_id);
        }
    }

    /// Returns the client's network info if the client exits.
    pub fn network_info(&self, client_id: u64) -> Option<NetworkInfo> {
        let addr = match self.netcode_server.client_addr(client_id) {
            Some(addr) => addr,
            None => return None,
        };

        let client_packet_info = self.clients_packet_info.get(&addr).unwrap();

        let sent_kbps = client_packet_info.sent_kbps;
        let received_kbps = client_packet_info.received_kbps;
        let rtt = self.reliable_server.client_rtt(client_id);
        let packet_loss = self.reliable_server.client_packet_loss(client_id);

        Some(NetworkInfo {
            received_kbps,
            sent_kbps,
            rtt,
            packet_loss,
        })
    }

    /// Advances the server by duration, and receive packets from the network.
    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        self.reliable_server.update_connections(duration);
        self.netcode_server.update(duration);

        let current_time = self.netcode_server.current_time();

        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if let Some(info) = self.clients_packet_info.get_mut(&addr) {
                        let packet_info = PacketInfo::new(current_time, len);
                        info.add_packet_received(packet_info);
                    }

                    let server_result = self.netcode_server.process_packet(addr, &mut self.buffer[..len]);
                    handle_server_result(
                        server_result,
                        current_time,
                        self.bandwidth_smoothing_factor,
                        &self.socket,
                        &mut self.reliable_server,
                        &mut self.clients_packet_info,
                        &mut self.events,
                    )?;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            };
        }

        for client_id in self.netcode_server.clients_id().into_iter() {
            let server_result = self.netcode_server.update_client(client_id);
            handle_server_result(
                server_result,
                current_time,
                self.bandwidth_smoothing_factor,
                &self.socket,
                &mut self.reliable_server,
                &mut self.clients_packet_info,
                &mut self.events,
            )?;
        }

        // Handle disconnected clients from Rechannel
        while let Some((client_id, reason)) = self.reliable_server.disconnected_client() {
            self.events.push_back(ServerEvent::ClientDisconnected(client_id));
            if reason != DisconnectionReason::DisconnectedByClient {
                match disconnect_packet(reason) {
                    Err(e) => error!("Failed to serialize disconnect packet: {}", e),
                    Ok(packet) => match self.netcode_server.generate_payload_packet(client_id, &packet) {
                        Err(e) => error!("Failed to encrypt disconnect packet: {}", e),
                        Ok((addr, payload)) => {
                            self.socket.send_to(payload, addr)?;
                        }
                    },
                }
            }
            self.netcode_server.disconnect(client_id);
        }

        for packet_info in self.clients_packet_info.values_mut() {
            packet_info.update_metrics();
        }

        Ok(())
    }

    /// Receive a message from a client over a channel.
    pub fn receive_message<I: Into<u8>>(&mut self, client_id: u64, channel_id: I) -> Option<Vec<u8>> {
        self.reliable_server.receive_message(&client_id, channel_id)
    }

    /// Verifies if a message can be sent to a client over a channel.
    pub fn can_send_message<I: Into<u8>>(&self, client_id: u64, channel_id: I) -> bool {
        self.reliable_server.can_send_message(&client_id, channel_id)
    }

    /// Send a message to a client over a channel.
    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, client_id: u64, channel_id: I, message: B) {
        self.reliable_server.send_message(&client_id, channel_id, message);
    }

    /// Send a message to all client, except the specified one, over a channel.
    pub fn broadcast_message_except<I: Into<u8>, B: Into<Bytes>>(&mut self, client_id: u64, channel_id: I, message: B) {
        self.reliable_server.broadcast_message_except(&client_id, channel_id, message)
    }

    /// Send a message to all client over a channel.
    pub fn broadcast_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        self.reliable_server.broadcast_message(channel_id, message);
    }

    /// Send packets to connected clients.
    pub fn send_packets(&mut self) -> Result<(), io::Error> {
        for client_id in self.reliable_server.connections_id().into_iter() {
            let packets = match self.reliable_server.get_packets_to_send(&client_id) {
                Ok(p) => p,
                Err(e) => {
                    error!("Failed to get packets from {}: {}", client_id, e);
                    continue;
                }
            };

            let current_time = self.netcode_server.current_time();
            for packet in packets.iter() {
                match self.netcode_server.generate_payload_packet(client_id, packet) {
                    Ok((addr, payload)) => {
                        send_to(current_time, &self.socket, &mut self.clients_packet_info, payload, addr)?;
                    }
                    Err(e) => error!("Failed to encrypt payload packet: {}", e),
                }
            }
        }

        Ok(())
    }

    /// Returns the client address if connected.
    pub fn client_addr(&self, client_id: u64) -> Option<SocketAddr> {
        self.netcode_server.client_addr(client_id)
    }

    /// Returns the user data from the connected client.
    pub fn user_data(&self, client_id: u64) -> Option<[u8; NETCODE_USER_DATA_BYTES]> {
        self.netcode_server.user_data(client_id)
    }

    pub fn is_client_connected(&self, client_id: u64) -> bool {
        self.netcode_server.is_client_connected(client_id)
    }

    /// Returns all the connected clients id.
    pub fn clients_id(&self) -> Vec<u64> {
        self.netcode_server.clients_id()
    }

    /// Returns the maximum number of clients that can be connected.
    pub fn max_clients(&self) -> usize {
        self.netcode_server.max_clients()
    }

    /// Returns the current number of clients connected.
    pub fn connected_clients(&self) -> usize {
        self.netcode_server.connected_clients()
    }
}

fn handle_server_result(
    server_result: ServerResult,
    current_time: Duration,
    bandwidth_smoothing_factor: f32,
    socket: &UdpSocket,
    reliable_server: &mut RechannelServer<u64>,
    packet_infos: &mut HashMap<SocketAddr, ClientPacketInfo>,
    events: &mut VecDeque<ServerEvent>,
) -> Result<(), io::Error> {
    match server_result {
        ServerResult::None => {}
        ServerResult::PacketToSend { payload, addr } => {
            send_to(current_time, socket, packet_infos, payload, addr)?;
        }
        ServerResult::Payload { client_id, payload } => {
            if !reliable_server.is_connected(&client_id) {
                reliable_server.add_connection(&client_id);
            }

            if let Err(e) = reliable_server.process_packet_from(payload, &client_id) {
                log::error!("Error while processing payload for {}: {}", client_id, e)
            }
        }
        ServerResult::ClientConnected {
            client_id,
            user_data,
            addr,
            payload,
        } => {
            reliable_server.add_connection(&client_id);
            packet_infos.insert(addr, ClientPacketInfo::new(bandwidth_smoothing_factor));
            events.push_back(ServerEvent::ClientConnected(client_id, user_data));
            send_to(current_time, socket, packet_infos, payload, addr)?;
        }
        ServerResult::ClientDisconnected { client_id, addr, payload } => {
            events.push_back(ServerEvent::ClientDisconnected(client_id));
            reliable_server.remove_connection(&client_id);
            packet_infos.remove(&addr);
            if let Some(payload) = payload {
                socket.send_to(payload, addr)?;
            }
        }
    }

    Ok(())
}

fn send_to(
    current_time: Duration,
    socket: &UdpSocket,
    packet_infos: &mut HashMap<SocketAddr, ClientPacketInfo>,
    packet: &[u8],
    addr: SocketAddr,
) -> Result<usize, std::io::Error> {
    if let Some(info) = packet_infos.get_mut(&addr) {
        let packet_info = PacketInfo::new(current_time, packet.len());
        info.add_packet_sent(packet_info);
    }
    socket.send_to(packet, addr)
}
