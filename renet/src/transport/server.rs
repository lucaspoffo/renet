use std::{
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renetcode::{NetcodeServer, ServerResult, NETCODE_KEY_BYTES, NETCODE_MAX_PACKET_BYTES, NETCODE_USER_DATA_BYTES};

use crate::server::RenetServer;

use super::NetcodeTransportError;

/// Configuration to establish a secure or unsecure connection with the server.
#[derive(Debug)]
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
#[derive(Debug)]
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

#[derive(Debug)]
pub struct NetcodeServerTransport {
    socket: UdpSocket,
    netcode_server: NetcodeServer,
    buffer: [u8; NETCODE_MAX_PACKET_BYTES],
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

    pub fn update(&mut self, duration: Duration, reliable_server: &mut RenetServer) -> Result<(), NetcodeTransportError> {
        self.netcode_server.update(duration);

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

        for client_id in self.netcode_server.clients_id() {
            let server_result = self.netcode_server.update_client(client_id);
            handle_server_result(server_result, &self.socket, reliable_server)?;
        }

        for disconnection_id in reliable_server.disconnections_id() {
            let server_result = self.netcode_server.disconnect(disconnection_id);
            handle_server_result(server_result, &self.socket, reliable_server)?;
        }

        Ok(())
    }

    /// Send packets to connected clients.
    pub fn send_packets(&mut self, reliable_server: &mut RenetServer) -> Result<(), io::Error> {
        for client_id in reliable_server.connections_id() {
            let packets = reliable_server.get_packets_to_send(client_id).unwrap();
            for packet in packets {
                match self.netcode_server.generate_payload_packet(client_id, &packet) {
                    Ok((addr, payload)) => {
                        self.socket.send_to(payload, addr)?;
                    }
                    Err(e) => log::error!("Failed to encrypt payload packet for client {}: {}", client_id, e),
                }
            }
        }

        Ok(())
    }
}

fn handle_server_result(server_result: ServerResult, socket: &UdpSocket, reliable_server: &mut RenetServer) -> Result<(), io::Error> {
    match server_result {
        ServerResult::None => {}
        ServerResult::PacketToSend { payload, addr } => {
            socket.send_to(payload, addr)?;
        }
        ServerResult::Payload { client_id, payload } => {
            if let Err(e) = reliable_server.process_packet_from(payload, client_id) {
                log::error!("Error while processing payload for {}: {}", client_id, e);
            }
        }
        ServerResult::ClientConnected {
            client_id,
            user_data: _,
            addr,
            payload,
        } => {
            reliable_server.add_connection(client_id);
            socket.send_to(payload, addr)?;
        }
        ServerResult::ClientDisconnected { client_id, addr, payload } => {
            reliable_server.remove_connection(client_id);
            if let Some(payload) = payload {
                socket.send_to(payload, addr)?;
            }
        }
    }

    Ok(())
}
