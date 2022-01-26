use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    packet::{ConnectionKeepAlive, ConnectionRequest, EncryptedChallengeToken, NetcodeError, Packet},
    token::PrivateConnectToken,
    NETCODE_KEY_BYTES, NETCODE_MAC_BYTES, NETCODE_VERSION_INFO, crypto::generate_random_bytes,
};

type ClientID = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Disconnected,
    PendingResponse,
    TimedOut,
    Connected,
}

#[derive(Debug, Clone)]
struct Connection {
    client_id: ClientID,
    state: ConnectionState,
    send_key: [u8; NETCODE_KEY_BYTES],
    receive_key: [u8; NETCODE_KEY_BYTES],
    addr: SocketAddr,
    last_packet_received_time: Duration,
    last_packet_send_time: Option<Duration>,
    timeout_seconds: i32,
    sequence: u64,
    // expire_timestamp: Duration, TODO
}

pub enum ServerEvent {
    ClientConnected(ClientID),
    ClientDisconnected(ClientID),
}

struct Server {
    clients: Box<[Option<Connection>]>,
    clients_pending_response: HashMap<SocketAddr, Connection>,
    protocol_id: u64,
    connect_key: [u8; NETCODE_KEY_BYTES],
    max_clients: usize,
    challenge_sequence: u64,
    challenge_key: [u8; NETCODE_KEY_BYTES],
    address: SocketAddr,
    current_time: Duration,
    events: VecDeque<ServerEvent>,
}

pub enum ServerResult<'a> {
    None,
    PacketToSend(Packet<'a>),
    Payload(&'a [u8]),
}

impl Server {
    pub fn new(max_clients: usize, protocol_id: u64, address: SocketAddr, private_key: [u8; NETCODE_KEY_BYTES]) -> Self {
        let challenge_key = generate_random_bytes();
        let clients = vec![None; max_clients].into_boxed_slice();

        Self {
            clients,
            clients_pending_response: HashMap::new(),
            protocol_id,
            connect_key: private_key,
            max_clients,
            challenge_sequence: 0,
            challenge_key,
            address,
            current_time: Duration::ZERO,
            events: VecDeque::new()
        }
    }

    pub fn handle_connection_request<'a>(
        &mut self,
        addr: SocketAddr,
        request: &ConnectionRequest,
    ) -> Result<ServerResult<'a>, NetcodeError> {
        let connect_token = self.validate_client_token(request)?;

        let id_already_connected = find_client_by_addr(&mut self.clients, addr).is_some();
        let addr_already_connected = find_client_by_id(&mut self.clients, connect_token.client_id).is_some();

        if id_already_connected || addr_already_connected {
            return Ok(ServerResult::None);
        }

        if self.clients.iter().flatten().count() >= self.max_clients {
            return Ok(ServerResult::PacketToSend(Packet::ConnectionDenied));
        }

        self.clients_pending_response.entry(addr).or_insert_with(|| Connection {
            sequence: 0,
            client_id: connect_token.client_id,
            last_packet_received_time: self.current_time,
            last_packet_send_time: Some(self.current_time),
            addr,
            state: ConnectionState::PendingResponse,
            send_key: connect_token.server_to_client_key,
            receive_key: connect_token.client_to_server_key,
            timeout_seconds: connect_token.timeout_seconds,
        });

        self.challenge_sequence += 1;
        let packet = Packet::Challenge(EncryptedChallengeToken::generate(
            connect_token.client_id,
            &connect_token.user_data,
            self.challenge_sequence,
            &self.challenge_key,
        )?);

        Ok(ServerResult::PacketToSend(packet))
    }

    pub fn validate_client_token(&self, request: &ConnectionRequest) -> Result<PrivateConnectToken, NetcodeError> {
        if request.version_info != *NETCODE_VERSION_INFO {
            return Err(NetcodeError::InvalidVersion);
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if now > request.expire_timestamp {
            return Err(NetcodeError::Expired);
        }

        let token = PrivateConnectToken::decode(
            &request.data,
            self.protocol_id,
            request.expire_timestamp,
            &request.xnonce,
            &self.connect_key,
        )?;

        let in_host_list = token.server_addresses.iter().any(|host| *host == Some(self.address));
        if in_host_list {
            Ok(token)
        } else {
            Err(NetcodeError::NotInHostList)
        }
    }

    fn process_packet<'a>(&mut self, addr: SocketAddr, buffer: &'a mut [u8]) -> ServerResult<'a> {
        match self.process_packet_internal(addr, buffer) {
            Err(_) => ServerResult::None,
            Ok(r) => r,
        }
    }

    fn process_packet_internal<'a>(&mut self, addr: SocketAddr, buffer: &'a mut [u8]) -> Result<ServerResult<'a>, NetcodeError> {
        if buffer.len() <= 2 + NETCODE_MAC_BYTES {
            return Ok(ServerResult::None);
        }

        let client = find_client_by_addr(&mut self.clients, addr);
        match client {
            Some(connection) => {
                let (_, packet) = Packet::decode(buffer, self.protocol_id, Some(&connection.receive_key))?;
                connection.last_packet_received_time = self.current_time;
                match connection.state {
                    ConnectionState::Connected => match packet {
                        Packet::Disconnect => {
                            connection.state = ConnectionState::Disconnected;

                            Ok(ServerResult::None)
                        }
                        Packet::Payload(payload) => Ok(ServerResult::Payload(payload)),
                        _ => Ok(ServerResult::None),
                    },
                    _ => Ok(ServerResult::None),
                }
            }
            None => match self.clients_pending_response.get_mut(&addr) {
                Some(pending) => {
                    let (_, packet) = Packet::decode(buffer, self.protocol_id, Some(&pending.receive_key))?;
                    pending.last_packet_received_time = self.current_time;
                    match packet {
                        Packet::ConnectionRequest(request) => self.handle_connection_request(addr, &request),
                        Packet::Response(response) => match pending.state {
                            ConnectionState::PendingResponse => {
                                response.decode(&self.challenge_key)?;
                                let pending = self.clients_pending_response.remove(&addr).unwrap();
                                match self.clients.iter().position(|c| c.is_none()) {
                                    None => Ok(ServerResult::PacketToSend(Packet::ConnectionDenied)),
                                    Some(client_index) => {
                                        self.events.push_back(ServerEvent::ClientConnected(pending.client_id));
                                        self.clients[client_index] = Some(pending);
                                        Ok(ServerResult::PacketToSend(Packet::KeepAlive(ConnectionKeepAlive {
                                            max_clients: self.max_clients as u32,
                                            client_index: client_index as u32,
                                        })))
                                    }
                                }
                            }
                            _ => Ok(ServerResult::None),
                        },
                        _ => Ok(ServerResult::None),
                    }
                }
                None => {
                    let (_, packet) = Packet::decode(buffer, self.protocol_id, None)?;
                    match packet {
                        Packet::ConnectionRequest(request) => self.handle_connection_request(addr, &request),
                        _ => Ok(ServerResult::None), // Decoding packet without key can only return ConnectionRequest
                    }
                }
            },
        }
    }

    pub fn encode_packet(&mut self, buffer: &mut [u8], addr: SocketAddr, packet: Packet<'_>) -> Result<usize, NetcodeError> {
        let connection = find_client_by_addr(&mut self.clients, addr).or(self.clients_pending_response.get_mut(&addr));
        match connection {
            Some(connection) => {
                let result = packet.encode(buffer, self.protocol_id, Some((connection.sequence, &connection.send_key)))?;
                connection.sequence += 1;
                Ok(result)
            }
            None => Err(NetcodeError::ClientNotFound),
        }
    }

    pub fn update(&mut self, duration: Duration) -> Vec<(SocketAddr, Packet<'_>)> {
        self.current_time += duration;
        let mut disconnect_packets = vec![];
        for maybe_client in self.clients.iter_mut() {
            if let Some(client) = maybe_client {
                let connection_timed_out = client.timeout_seconds > 0
                    && (client.last_packet_received_time + Duration::from_secs(client.timeout_seconds as u64) < self.current_time);
                if connection_timed_out {
                    client.state = ConnectionState::Disconnected;
                }

                if client.state == ConnectionState::Disconnected {
                    self.events.push_back(ServerEvent::ClientDisconnected(client.client_id));
                    disconnect_packets.push((client.addr, Packet::Disconnect));
                    *maybe_client = None;
                }
            }
        }

        disconnect_packets
    }
}

fn find_client_by_id(clients: &mut [Option<Connection>], id: ClientID) -> Option<&mut Connection> {
    clients.iter_mut().flatten().find(|c| c.client_id == id)
}

fn find_client_by_addr(clients: &mut [Option<Connection>], addr: SocketAddr) -> Option<&mut Connection> {
    clients.iter_mut().flatten().find(|c| c.addr == addr)
}

#[cfg(test)]
mod tests {
    use crate::{token::ConnectToken, client::Client, NETCODE_BUFFER_SIZE};

    use super::*;

    #[test]
    fn server_connection() {
        let protocol_id = 7;
        let max_clients = 16;
        let server_addr = "127.0.0.1:5000".parse().unwrap();
        let private_key = b"an example very very secret key."; // 32-bytes
        let mut server = Server::new(max_clients, protocol_id, server_addr, *private_key);

        let server_addresses: Vec<SocketAddr> = vec![server_addr];
        let user_data = generate_random_bytes();
        let expire_seconds = 3;
        let client_id = 4;
        let timeout_seconds = 5;
        let client_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let connect_token = ConnectToken::generate(
            protocol_id,
            expire_seconds,
            client_id,
            timeout_seconds,
            server_addresses,
            Some(&user_data),
            private_key,
        )
        .unwrap();
        let mut client = Client::new(Duration::ZERO, connect_token);

        let mut buffer = [0u8; NETCODE_BUFFER_SIZE];

        // let packet_request = client.generate_packet().unwrap();

        // let challenge_packet = server.process_packet(client_addr, &mut buffer);


    }
}
