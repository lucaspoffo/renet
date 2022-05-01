use rechannel::{
    disconnect_packet,
    error::{DisconnectionReason, RechannelError},
    server::RechannelServer,
};

use renetcode::{NetcodeServer, PacketToSend, ServerResult, NETCODE_KEY_BYTES, NETCODE_USER_DATA_BYTES};

use log::error;
use std::{
    collections::VecDeque,
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use crate::{RenetConnectionConfig, NUM_DISCONNECT_PACKETS_TO_SEND};

/// A server that can establish authenticated connections with multiple clients.
/// Can send/receive encrypted messages from/to them.
#[derive(Debug)]
pub struct RenetServer {
    socket: UdpSocket,
    reliable_server: RechannelServer<u64>,
    netcode_server: NetcodeServer,
    buffer: Box<[u8]>,
    events: VecDeque<ServerEvent>,
}

/// Events that can occur in the server.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // TODO: Consider boxing types
pub enum ServerEvent {
    ClientConnected(u64, [u8; NETCODE_USER_DATA_BYTES]),
    ClientDisconnected(u64),
}

/// Configuration options for the renet server.
pub struct ServerConfig {
    /// Maximum numbers of clients that can be connected at a time
    pub max_clients: usize,
    /// Unique id to this game/application
    pub protocol_id: u64,
    /// The server address
    pub server_addr: SocketAddr,
    /// Private key used for encryption in the server
    pub private_key: [u8; NETCODE_KEY_BYTES],
}

impl ServerConfig {
    pub fn new(max_clients: usize, protocol_id: u64, server_addr: SocketAddr, private_key: [u8; NETCODE_KEY_BYTES]) -> Self {
        Self {
            max_clients,
            protocol_id,
            server_addr,
            private_key,
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
        let reliable_server = RechannelServer::new(connection_config.to_connection_config());
        let netcode_server = NetcodeServer::new(
            current_time,
            server_config.max_clients,
            server_config.protocol_id,
            server_config.server_addr,
            server_config.private_key,
        );

        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            netcode_server,
            reliable_server,
            buffer,
            events: VecDeque::new(),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.netcode_server.address()
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    /// Disconnects a client.
    pub fn disconnect(&mut self, client_id: u64) {
        let server_result = self.netcode_server.disconnect(client_id);
        if let Err(e) = handle_server_result(server_result, &self.socket, &mut self.reliable_server, &mut self.events) {
            error!("Failed to send disconnect packet to client {}: {}", client_id, e);
        }
    }

    /// Disconnects all connected clients.
    pub fn disconnect_clients(&mut self) {
        for client_id in self.netcode_server.clients_id() {
            self.disconnect(client_id);
        }
    }

    /// Advances the server by duration, and receive packets from the network.
    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    let server_result = self.netcode_server.process_packet(addr, &mut self.buffer[..len]);
                    handle_server_result(server_result, &self.socket, &mut self.reliable_server, &mut self.events)?;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            };
        }

        self.reliable_server.update_connections(duration);
        self.netcode_server.update(duration);

        for client_id in self.netcode_server.clients_id().into_iter() {
            let server_result = self.netcode_server.update_client(client_id);
            handle_server_result(server_result, &self.socket, &mut self.reliable_server, &mut self.events)?;
        }

        // Handle disconnected clients from Rechannel
        while let Some((client_id, reason)) = self.reliable_server.disconnected_client() {
            self.events.push_back(ServerEvent::ClientDisconnected(client_id));
            if reason != DisconnectionReason::DisconnectedByClient {
                match disconnect_packet(reason) {
                    Err(e) => error!("failed to serialize disconnect packet: {}", e),
                    Ok(packet) => match self.netcode_server.generate_payload_packet(client_id, &packet) {
                        Err(e) => error!("failed to encrypt disconnect packet: {}", e),
                        Ok(PacketToSend { packet, address }) => {
                            for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                                self.socket.send_to(packet, address)?;
                            }
                        }
                    },
                }
            }
            self.netcode_server.disconnect(client_id);
        }

        Ok(())
    }

    /// Receive a message from a client over a channel.
    pub fn receive_message(&mut self, client_id: u64, channel_id: u8) -> Option<Vec<u8>> {
        self.reliable_server.receive_message(&client_id, channel_id)
    }

    /// Send a message from a clients over a channel.
    pub fn send_message(&mut self, client_id: u64, channel_id: u8, message: Vec<u8>) -> Result<(), RechannelError> {
        self.reliable_server.send_message(&client_id, channel_id, message)
    }

    /// Send a message to all client, except the specified one, over a channel.
    pub fn broadcast_message_except(&mut self, client_id: u64, channel_id: u8, message: Vec<u8>) {
        self.reliable_server.broadcast_message_except(&client_id, channel_id, message)
    }

    /// Send a message to all client over a channel.
    pub fn broadcast_message(&mut self, channel_id: u8, message: Vec<u8>) {
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

            for packet in packets.iter() {
                match self.netcode_server.generate_payload_packet(client_id, packet) {
                    Ok(PacketToSend { packet, address }) => {
                        self.socket.send_to(packet, address)?;
                    }
                    Err(e) => error!("failed to encrypt payload packet: {}", e),
                }
            }
        }

        Ok(())
    }

    /// Returns all the connected clients id.
    pub fn clients_id(&self) -> Vec<u64> {
        self.netcode_server.clients_id()
    }
}

fn handle_server_result(
    server_result: ServerResult,
    socket: &UdpSocket,
    reliable_server: &mut RechannelServer<u64>,
    events: &mut VecDeque<ServerEvent>,
) -> Result<(), io::Error> {
    match server_result {
        ServerResult::None => {}
        ServerResult::PacketToSend(PacketToSend { packet, address }) => {
            socket.send_to(packet, address)?;
        }
        ServerResult::Payload(client_id, payload) => {
            if !reliable_server.is_connected(&client_id) {
                reliable_server.add_connection(&client_id);
            }
            if let Err(e) = reliable_server.process_packet_from(payload, &client_id) {
                log::error!("Error while processing payload for {}: {}", client_id, e)
            }
        }
        ServerResult::ClientConnected(client_id, user_data, PacketToSend { packet, address }) => {
            reliable_server.add_connection(&client_id);
            events.push_back(ServerEvent::ClientConnected(client_id, user_data));
            socket.send_to(packet, address)?;
        }
        ServerResult::ClientDisconnected(client_id, packet_to_send) => {
            events.push_back(ServerEvent::ClientDisconnected(client_id));
            reliable_server.remove_connection(&client_id);
            if let Some(PacketToSend { packet, address }) = packet_to_send {
                for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                    socket.send_to(packet, address)?;
                }
            }
        }
    }

    Ok(())
}
