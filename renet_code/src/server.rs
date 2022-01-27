use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    crypto::generate_random_bytes,
    packet::{ConnectionKeepAlive, ConnectionRequest, EncryptedChallengeToken, NetcodeError, Packet},
    token::PrivateConnectToken,
    NETCODE_KEY_BYTES, NETCODE_MAC_BYTES, NETCODE_VERSION_INFO,
};

type ClientID = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Disconnected,
    PendingResponse,
    Connected,
}

// TODO: Replay Protection
// TODO: Check what client confirmed is used for connected clients.
//       A client is considered confirmed after he receives an 
//       payload or keep alive packet after being connected
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
    expire_timestamp: u64,
    create_timestamp: u64,
    connect_start_time: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ServerEvent {
    ClientConnected(usize),
    ClientDisconnected(usize),
}

pub struct Server {
    clients: Box<[Option<Connection>]>,
    pending_clients: HashMap<SocketAddr, Connection>,
    protocol_id: u64,
    connect_key: [u8; NETCODE_KEY_BYTES],
    max_clients: usize,
    challenge_sequence: u64,
    challenge_key: [u8; NETCODE_KEY_BYTES],
    address: SocketAddr,
    current_time: Duration,
    global_sequence: u64,
    events: VecDeque<ServerEvent>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ServerResult<'a, 'b> {
    None,
    PacketToSend(&'b mut [u8]),
    Payload(&'a [u8]),
}

impl Server {
    pub fn new(max_clients: usize, protocol_id: u64, address: SocketAddr, private_key: [u8; NETCODE_KEY_BYTES]) -> Self {
        let challenge_key = generate_random_bytes();
        let clients = vec![None; max_clients].into_boxed_slice();

        Self {
            clients,
            pending_clients: HashMap::new(),
            protocol_id,
            connect_key: private_key,
            max_clients,
            challenge_sequence: 0,
            global_sequence: 0,
            challenge_key,
            address,
            current_time: Duration::ZERO,
            events: VecDeque::new(),
        }
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    pub fn handle_connection_request<'a, 'b>(
        &mut self,
        addr: SocketAddr,
        request: &ConnectionRequest,
        out: &'b mut [u8],
    ) -> Result<ServerResult<'a, 'b>, NetcodeError> {
        let connect_token = self.validate_client_token(request)?;

        let id_already_connected = find_client_by_addr(&mut self.clients, addr).is_some();
        let addr_already_connected = find_client_by_id(&mut self.clients, connect_token.client_id).is_some();

        if id_already_connected || addr_already_connected {
            return Ok(ServerResult::None);
        }

        if self.clients.iter().flatten().count() >= self.max_clients {
            self.pending_clients.remove(&addr);
            let packet = Packet::ConnectionDenied;
            let len = packet.encode(
                out,
                self.protocol_id,
                Some((self.global_sequence, &connect_token.server_to_client_key)),
            )?;
            self.global_sequence += 1;
            return Ok(ServerResult::PacketToSend(&mut out[..len]));
        }

        self.pending_clients.entry(addr).or_insert_with(|| Connection {
            sequence: 0,
            client_id: connect_token.client_id,
            last_packet_received_time: self.current_time,
            last_packet_send_time: Some(self.current_time),
            addr,
            state: ConnectionState::PendingResponse,
            send_key: connect_token.server_to_client_key,
            receive_key: connect_token.client_to_server_key,
            timeout_seconds: connect_token.timeout_seconds,
            connect_start_time: self.current_time,
            expire_timestamp: request.expire_timestamp,
            create_timestamp: request.create_timestamp,
        });

        self.challenge_sequence += 1;
        let packet = Packet::Challenge(EncryptedChallengeToken::generate(
            connect_token.client_id,
            &connect_token.user_data,
            self.challenge_sequence,
            &self.challenge_key,
        )?);
        let len = packet.encode(
            out,
            self.protocol_id,
            Some((self.global_sequence, &connect_token.server_to_client_key)),
        )?;
        self.global_sequence += 1;
        Ok(ServerResult::PacketToSend(&mut out[..len]))
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

    pub fn generate_payload<'a>(&mut self, slot: usize, payload: &[u8], out: &'a mut [u8]) -> Result<&'a mut [u8], NetcodeError> {
        if slot >= self.clients.len() {
            return Err(NetcodeError::ClientNotFound);
        }

        if let Some(client) = &mut self.clients[slot] {
            let packet = Packet::Payload(payload);
            let len = packet.encode(out, self.protocol_id, Some((client.sequence, &client.send_key)))?;
            client.sequence += 1;
            return Ok(&mut out[..len]);
        }

        Err(NetcodeError::ClientNotFound)
    }

    pub fn process_packet<'a, 'b>(&mut self, addr: SocketAddr, buffer: &'a mut [u8], out: &'b mut [u8]) -> ServerResult<'a, 'b> {
        match self.process_packet_internal(addr, buffer, out) {
            Err(_) => ServerResult::None,
            Ok(r) => r,
        }
    }

    fn process_packet_internal<'a, 'b>(
        &mut self,
        addr: SocketAddr,
        buffer: &'a mut [u8],
        out: &'b mut [u8],
    ) -> Result<ServerResult<'a, 'b>, NetcodeError> {
        if buffer.len() <= 2 + NETCODE_MAC_BYTES {
            return Ok(ServerResult::None);
        }

        // Handle connected client
        if let Some((slot, connection)) = find_client_by_addr(&mut self.clients, addr) {
            let (_, packet) = Packet::decode(buffer, self.protocol_id, Some(&connection.receive_key))?;
            connection.last_packet_received_time = self.current_time;
            match connection.state {
                ConnectionState::Connected => match packet {
                    Packet::Disconnect => {
                        connection.state = ConnectionState::Disconnected;
                        self.events.push_back(ServerEvent::ClientDisconnected(slot));
                        self.clients[slot] = None;
                        return Ok(ServerResult::None);
                    }
                    Packet::Payload(payload) => return Ok(ServerResult::Payload(payload)),
                    _ => return Ok(ServerResult::None),
                },
                _ => return Ok(ServerResult::None),
            }
        }

        // Handle pending client
        if let Some(pending) = self.pending_clients.get_mut(&addr) {
            let (_, packet) = Packet::decode(buffer, self.protocol_id, Some(&pending.receive_key))?;
            pending.last_packet_received_time = self.current_time;
            match packet {
                Packet::ConnectionRequest(request) => return self.handle_connection_request(addr, &request, out),
                Packet::Response(response) => {
                    response.decode(&self.challenge_key)?;
                    let mut pending = self.pending_clients.remove(&addr).unwrap();
                    match self.clients.iter().position(|c| c.is_none()) {
                        None => {
                            let packet = Packet::ConnectionDenied;
                            let len = packet.encode(out, self.protocol_id, Some((self.global_sequence, &pending.send_key)))?;
                            pending.state = ConnectionState::Disconnected;
                            self.global_sequence += 1;
                            return Ok(ServerResult::PacketToSend(&mut out[..len]));
                        }
                        Some(client_index) => {
                            self.events.push_back(ServerEvent::ClientConnected(client_index));
                            let send_key = pending.send_key;
                            pending.state = ConnectionState::Connected;
                            self.clients[client_index] = Some(pending);
                            let packet = Packet::KeepAlive(ConnectionKeepAlive {
                                max_clients: self.max_clients as u32,
                                client_index: client_index as u32,
                            });
                            let len = packet.encode(out, self.protocol_id, Some((self.global_sequence, &send_key)))?;
                            self.global_sequence += 1;
                            return Ok(ServerResult::PacketToSend(&mut out[..len]));
                        }
                    }
                }
                _ => return Ok(ServerResult::None),
            }
        }

        // Handle new client
        let (_, packet) = Packet::decode(buffer, self.protocol_id, None)?;
        match packet {
            Packet::ConnectionRequest(request) => self.handle_connection_request(addr, &request, out),
            _ => Ok(ServerResult::None), // Decoding packet without key can only return ConnectionRequest
        }
    }

    pub fn update(&mut self, duration: Duration) -> Vec<(SocketAddr, Packet<'_>)> {
        self.current_time += duration;
        let mut disconnect_packets = vec![];
        for (slot, maybe_client) in self.clients.iter_mut().enumerate() {
            if let Some(client) = maybe_client {
                let connection_timed_out = client.timeout_seconds > 0
                    && (client.last_packet_received_time + Duration::from_secs(client.timeout_seconds as u64) < self.current_time);
                if connection_timed_out {
                    client.state = ConnectionState::Disconnected;
                }

                if client.state == ConnectionState::Disconnected {
                    self.events.push_back(ServerEvent::ClientDisconnected(slot));
                    disconnect_packets.push((client.addr, Packet::Disconnect));
                    *maybe_client = None;
                }
            }
        }

        disconnect_packets
    }

    pub fn clients_slot(&self) -> Vec<usize> {
        self.clients
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| if slot.is_some() { Some(index) } else { None })
            .collect()
    }

    pub fn clients_id(&self) -> Vec<ClientID> {
        self.clients
            .iter()
            .filter_map(|slot| slot.as_ref().map(|client| client.client_id))
            .collect()
    }

    pub fn max_clients(&self) -> usize {
        self.max_clients
    }

    pub fn update_client(&mut self, buffer: &mut [u8], slot: usize) -> Option<(usize, SocketAddr)> {
        if slot >= self.clients.len() {
            return None;
        }

        if let Some(client) = &mut self.clients[slot] {
            let connection_timed_out = client.timeout_seconds > 0
                && (client.last_packet_received_time + Duration::from_secs(client.timeout_seconds as u64) < self.current_time);
            if connection_timed_out {
                client.state = ConnectionState::Disconnected;
            }

            if client.state == ConnectionState::Disconnected {
                self.events.push_back(ServerEvent::ClientDisconnected(slot));
                let packet = Packet::Disconnect;
                let sequence = client.sequence;
                let send_key = client.send_key;
                let addr = client.addr;
                self.clients[slot] = None;
                let len = match packet.encode(buffer, self.protocol_id, Some((sequence, &send_key))) {
                    Err(_) => return None,
                    Ok(len) => len,
                };
                return Some((len, addr));
            }
        }

        None
    }

    pub fn update_pending_connections(&mut self) {
        for client in self.pending_clients.values_mut() {
            let expire_seconds = client.expire_timestamp - client.create_timestamp;
            let connection_expired = (self.current_time - client.connect_start_time).as_secs() >= expire_seconds;
            if connection_expired {
                client.state = ConnectionState::Disconnected;
            }
        }

        self.pending_clients.retain(|_, c| c.state != ConnectionState::Disconnected);
    }
}

fn find_client_by_id(clients: &mut [Option<Connection>], id: ClientID) -> Option<&mut Connection> {
    clients.iter_mut().flatten().find(|c| c.client_id == id)
}

fn find_client_by_addr(clients: &mut [Option<Connection>], addr: SocketAddr) -> Option<(usize, &mut Connection)> {
    clients.iter_mut().enumerate().find_map(|(i, c)| match c {
        Some(c) if c.addr == addr => Some((i, c)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::{client::Client, token::ConnectToken, NETCODE_BUFFER_SIZE};

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
        let mut out = [0u8; NETCODE_BUFFER_SIZE];

        let len = client.generate_packet(&mut buffer).unwrap();

        let result = server.process_packet(client_addr, &mut buffer[..len], &mut out);
        assert!(matches!(result, ServerResult::PacketToSend(_)));
        match result {
            ServerResult::PacketToSend(packet) => client.process_packet(packet),
            _ => unreachable!(),
        };
        let len = client.generate_packet(&mut buffer).unwrap();
        let result = server.process_packet(client_addr, &mut buffer[..len], &mut out);
        assert!(matches!(result, ServerResult::PacketToSend(_)));

        let client_connected = server.get_event().unwrap();
        assert_eq!(client_connected, ServerEvent::ClientConnected(0));

        assert!(!client.connected());
        match result {
            ServerResult::PacketToSend(packet) => client.process_packet(packet),
            _ => unreachable!(),
        };
        assert!(client.connected());

        let payload = [7u8; 300];
        let result = server.generate_payload(0, &payload, &mut out).unwrap();

        let client_payload = client.process_packet(result).unwrap();
        assert_eq!(payload, client_payload);
    }
}
