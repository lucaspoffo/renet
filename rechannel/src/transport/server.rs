use std::{
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renetcode::{NetcodeServer, ServerResult, NETCODE_KEY_BYTES, NETCODE_MAX_PACKET_BYTES, NETCODE_USER_DATA_BYTES};

use crate::{
    error::DisconnectionReason,
    serialize_disconnect_packet,
    server::{RechannelServer, ServerEvent},
};

use super::{NetcodeTransportError, NUM_DISCONNECT_PACKETS_TO_SEND, ServerTransport};

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

pub struct NetcodeServerTransport {
    socket: UdpSocket,
    netcode_server: NetcodeServer,
    buffer: [u8; NETCODE_MAX_PACKET_BYTES],
}

impl NetcodeServerTransport {
    pub fn new(current_time: Duration, server_config: ServerConfig, socket: UdpSocket) -> Result<Self, std::io::Error> {
        socket.set_nonblocking(true)?;

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

        Ok(Self {
            socket,
            netcode_server,
            buffer: [0; NETCODE_MAX_PACKET_BYTES],
        })
    }

    pub fn user_data(&self, client_id: u64) -> Option<[u8; NETCODE_USER_DATA_BYTES]> {
        self.netcode_server.user_data(client_id)
    }

    pub fn update(&mut self, duration: Duration, reliable_server: &mut RechannelServer) -> Result<(), NetcodeTransportError> {
        self.netcode_server.update(duration);

        // Handle disconnected clients from Renet
        for event in reliable_server.events() {
            let ServerEvent::ClientDisconnected { client_id, reason } = event else {
                continue;
            };


            if !self.netcode_server.is_client_connected(*client_id) {
                continue;
            }
            
            if *reason != DisconnectionReason::DisconnectedByClient {
                match serialize_disconnect_packet(*reason) {
                    Err(e) => log::error!("Failed to serialize disconnect packet: {}", e),
                    Ok(packet) => match self.netcode_server.generate_payload_packet(*client_id, &packet) {
                        Err(e) => log::error!("Failed to encrypt disconnect packet: {}", e),
                        Ok((addr, payload)) => {
                            for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                                self.socket.send_to(payload, addr)?;
                            }
                        }
                    },
                }
            }
           
            self.netcode_server.disconnect(*client_id);
        }

        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    let server_result = self.netcode_server.process_packet(addr, &mut self.buffer[..len]);
                    handle_server_result(server_result, &self.socket, reliable_server)?;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };
        }

        for client_id in self.netcode_server.clients_id().into_iter() {
            let server_result = self.netcode_server.update_client(client_id);
            handle_server_result(server_result, &self.socket, reliable_server)?;
        }

        Ok(())
    }
}

fn handle_server_result(server_result: ServerResult, socket: &UdpSocket, reliable_server: &mut RechannelServer) -> Result<(), io::Error> {
    match server_result {
        ServerResult::None => {}
        ServerResult::PacketToSend { payload, addr } => {
            socket.send_to(payload, addr)?;
        }
        ServerResult::Payload { client_id, payload } => {
            if let Err(e) = reliable_server.process_packet_from(payload, client_id) {
                log::error!("Error while processing payload for {}: {}", client_id, e)
            }
        }
        ServerResult::ClientConnected {
            client_id,
            user_data: _,
            addr,
            payload,
        } => {
            reliable_server.add_client(client_id);
            socket.send_to(payload, addr)?;
        }
        ServerResult::ClientDisconnected { client_id, addr, payload } => {
            reliable_server.disconnect_with_reason(client_id, DisconnectionReason::CustomError { code: 0 });
            if let Some(payload) = payload {
                for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                    socket.send_to(payload, addr)?;
                }
            }
        }
    }

    Ok(())
}

impl ServerTransport for NetcodeServerTransport {
    fn send_to(&mut self, client_id: u64, packet: &[u8]) {
        match self.netcode_server.generate_payload_packet(client_id, packet) {
            Ok((addr, payload)) => {
                if let Err(e) = self.socket.send_to(payload, addr) {
                    log::error!("Failed to send packet to client {}: {}", addr, e);
                }
            }
            Err(e) => log::error!("Failed to encrypt payload packet: {}", e),
        }
    }
}