use std::{io, net::SocketAddr, time::Duration};

use renetcode::{NetcodeServer, ServerConfig, ServerResult, NETCODE_MAX_PACKET_BYTES, NETCODE_USER_DATA_BYTES};

use crate::ClientId;
use crate::RenetServer;

use super::{NetcodeTransportError, TransportSocket};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct NetcodeServerTransport {
    sockets: Vec<Box<dyn TransportSocket>>,
    netcode_server: NetcodeServer,
    buffer: [u8; NETCODE_MAX_PACKET_BYTES],
}

impl NetcodeServerTransport {
    /// Makes a new server transport that uses `netcode` for managing connections and data flow.
    pub fn new(server_config: ServerConfig, socket: impl TransportSocket) -> Result<Self, std::io::Error> {
        Self::new_with_sockets(server_config, vec![Box::new(socket)])
    }

    /// Makes a new server transport that uses `netcode` for managing connections and data flow.
    ///
    /// Multiple [`TransportSockets`](super::TransportSocket) may be inserted. Each socket must line
    /// up 1:1 with socket config entries in `ServerConfig::sockets`.
    pub fn new_with_sockets(
        server_config: ServerConfig,
        sockets: Vec<Box<dyn TransportSocket>>
    ) -> Result<Self, std::io::Error> {
        if server_config.sockets.len() == 0 {
            panic!("netcode server transport must have at least 1 socket");
        }
        if server_config.sockets.len() != sockets.len() {
            panic!("server config does not match the number of sockets");
        }

        Ok(Self {
            sockets,
            netcode_server: NetcodeServer::new(server_config),
            buffer: [0; NETCODE_MAX_PACKET_BYTES],
        })
    }

    /// Returns the server's public addresses for the first transport socket.
    pub fn addresses(&self) -> Vec<SocketAddr> {
        self.get_addresses(0).unwrap()
    }

    /// Returns the server's public addresses for a given `socket_id`.
    pub fn get_addresses(&self, socket_id: usize) -> Option<Vec<SocketAddr>> {
        if socket_id >= self.sockets.len() {
            return None;
        }
        Some(self.netcode_server.addresses(socket_id))
    }

    /// Returns the maximum number of clients that can be connected.
    pub fn max_clients(&self) -> usize {
        self.netcode_server.max_clients()
    }

    /// Returns current number of clients connected.
    pub fn connected_clients(&self) -> usize {
        self.netcode_server.connected_clients()
    }

    /// Returns the user data for client if connected.
    pub fn user_data(&self, client_id: ClientId) -> Option<[u8; NETCODE_USER_DATA_BYTES]> {
        self.netcode_server.user_data(client_id.raw())
    }

    /// Returns the client socket id and address if connected.
    pub fn client_addr(&self, client_id: ClientId) -> Option<(usize, SocketAddr)> {
        self.netcode_server.client_addr(client_id.raw())
    }

    /// Disconnects all connected clients.
    ///
    /// This sends the disconnect packet instantly, use this when closing/exiting games,
    /// should use [RenetServer::disconnect_all][crate::RenetServer::disconnect_all] otherwise.
    pub fn disconnect_all(&mut self, server: &mut RenetServer) {
        for client_id in self.netcode_server.clients_id() {
            let server_result = self.netcode_server.disconnect(client_id);
            let ServerResult::ClientDisconnected{ socket_id, ..} = &server_result else {
                continue;
            };
            let socket_id = *socket_id;
            handle_server_result(server_result, socket_id, &mut self.sockets[socket_id], server);
        }
    }

    /// Returns the duration since the connected client last received a packet.
    ///
    /// Useful to detect users that are timing out.
    pub fn time_since_last_received_packet(&self, client_id: ClientId) -> Option<Duration> {
        self.netcode_server.time_since_last_received_packet(client_id.raw())
    }

    /// Advances the transport by the duration, and receive packets from the network.
    pub fn update(&mut self, duration: Duration, server: &mut RenetServer) -> Result<(), NetcodeTransportError> {
        self.netcode_server.update(duration);

        for (socket_id, mut socket) in self.sockets.iter_mut().enumerate() {
            socket.preupdate();

            loop {
                match socket.try_recv(&mut self.buffer) {
                    Ok((len, addr)) => {
                        let server_result = self.netcode_server.process_packet(socket_id, addr, &mut self.buffer[..len]);
                        handle_server_result(server_result, socket_id, &mut socket, server);
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => break,
                    Err(ref e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                    Err(e) => return Err(e.into()),
                };
            }

            for client_id in self.netcode_server.clients_id() {
                let server_result = self.netcode_server.update_client(client_id);
                handle_server_result(server_result, socket_id, &mut socket, server);
            }

            for disconnection_id in server.disconnections_id() {
                let server_result = self.netcode_server.disconnect(disconnection_id.raw());
                handle_server_result(server_result, socket_id, &mut socket, server);
            }

            socket.postupdate();
        }

        Ok(())
    }

    /// Sends packets to connected clients.
    pub fn send_packets(&mut self, server: &mut RenetServer) {
        'clients: for client_id in server.clients_id() {
            let packets = server.get_packets_to_send(client_id).unwrap();
            for packet in packets {
                match self.netcode_server.generate_payload_packet(client_id.raw(), &packet) {
                    Ok((socket_id, addr, payload)) => {
                        if let Err(e) = self.sockets[socket_id].send(addr, payload) {
                            log::error!("Failed to send packet to client {client_id} ({socket_id}/{addr}): {e}");
                            continue 'clients;
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to encrypt payload packet for client {client_id}: {e}");
                        continue 'clients;
                    }
                }
            }
        }
    }
}

fn handle_server_result(
    server_result: ServerResult,
    socket_id: usize,
    socket: &mut Box<dyn TransportSocket>,
    reliable_server: &mut RenetServer
) {
    let mut send_packet = |packet: &[u8], s_id: usize, addr: SocketAddr| {
        if s_id != socket_id {
            log::error!("Tried to send a packet to the wrong socket id (found: {s_id}, expected: {socket_id})");
            return;
        }
        if let Err(err) = socket.send(addr, packet) {
            log::error!("Failed to send packet to {socket_id}/{addr}: {err}");
        }
    };

    match server_result {
        ServerResult::None => {}
        ServerResult::PacketToSend { payload, addr, socket_id } => {
            send_packet(payload, socket_id, addr);
        }
        ServerResult::Payload { client_id, payload } => {
            let client_id = ClientId::from_raw(client_id);
            if let Err(e) = reliable_server.process_packet_from(payload, client_id) {
                log::error!("Error while processing payload for {}: {}", client_id, e);
            }
        }
        ServerResult::ClientConnected {
            client_id,
            user_data: _,
            addr,
            payload,
            socket_id,
        } => {
            reliable_server.add_connection(ClientId::from_raw(client_id));
            send_packet(payload, socket_id, addr);
        }
        ServerResult::ClientDisconnected { client_id, addr, payload, socket_id } => {
            reliable_server.remove_connection(ClientId::from_raw(client_id));
            if let Some(payload) = payload {
                send_packet(payload, socket_id, addr);
            }
            socket.disconnect(addr);
        }
    }
}
