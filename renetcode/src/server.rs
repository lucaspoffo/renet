use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::Duration,
};

use crate::{
    crypto::generate_random_bytes,
    packet::{ConnectionKeepAlive, ConnectionRequest, EncryptedChallengeToken, Packet},
    replay_protection::ReplayProtection,
    token::PrivateConnectToken,
    NetcodeError, NETCODE_CONNECT_TOKEN_PRIVATE_BYTES, NETCODE_KEY_BYTES, NETCODE_MAC_BYTES, NETCODE_MAX_CLIENTS, NETCODE_MAX_PACKET_BYTES,
    NETCODE_MAX_PAYLOAD_BYTES, NETCODE_MAX_PENDING_CLIENTS, NETCODE_SEND_RATE, NETCODE_USER_DATA_BYTES, NETCODE_VERSION_INFO,
};

type ClientID = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Disconnected,
    PendingResponse,
    Connected,
}

#[derive(Debug, Clone)]
struct Connection {
    confirmed: bool,
    client_id: ClientID,
    state: ConnectionState,
    send_key: [u8; NETCODE_KEY_BYTES],
    receive_key: [u8; NETCODE_KEY_BYTES],
    user_data: [u8; NETCODE_USER_DATA_BYTES],
    addr: SocketAddr,
    last_packet_received_time: Duration,
    last_packet_send_time: Duration,
    timeout_seconds: i32,
    sequence: u64,
    expire_timestamp: u64,
    replay_protection: ReplayProtection,
}

#[derive(Debug, Copy, Clone)]
struct ConnectTokenEntry {
    time: Duration,
    address: SocketAddr,
    mac: [u8; NETCODE_MAC_BYTES],
}

#[derive(Debug, PartialEq, Eq)]
pub enum ServerEvent {
    ClientConnected(ClientID),
    ClientDisconnected(ClientID),
}

pub struct Server {
    clients: Box<[Option<Connection>]>,
    pending_clients: HashMap<SocketAddr, Connection>,
    connect_token_entries: [Option<ConnectTokenEntry>; NETCODE_MAX_CLIENTS * 4],
    protocol_id: u64,
    connect_key: [u8; NETCODE_KEY_BYTES],
    max_clients: usize,
    challenge_sequence: u64,
    challenge_key: [u8; NETCODE_KEY_BYTES],
    address: SocketAddr,
    current_time: Duration,
    global_sequence: u64,
    events: VecDeque<ServerEvent>,
    out: [u8; NETCODE_MAX_PACKET_BYTES],
}

#[derive(Debug, PartialEq, Eq)]
pub enum ServerResult<'a, 's> {
    None,
    PacketToSend(&'s mut [u8]),
    Payload(usize, &'a [u8]),
}

impl Server {
    pub fn new(
        current_time: Duration,
        max_clients: usize,
        protocol_id: u64,
        address: SocketAddr,
        private_key: [u8; NETCODE_KEY_BYTES],
    ) -> Self {
        if max_clients > NETCODE_MAX_CLIENTS {
            // TODO: do we really need to set a max?
            //       only using for token entries
            panic!("The max clients allowed is {}", NETCODE_MAX_CLIENTS);
        }
        let challenge_key = generate_random_bytes();
        let clients = vec![None; max_clients].into_boxed_slice();

        Self {
            clients,
            connect_token_entries: [None; NETCODE_MAX_CLIENTS * 4],
            pending_clients: HashMap::new(),
            protocol_id,
            connect_key: private_key,
            max_clients,
            challenge_sequence: 0,
            global_sequence: 0,
            challenge_key,
            address,
            current_time,
            events: VecDeque::new(),
            out: [0u8; NETCODE_MAX_PACKET_BYTES],
        }
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    fn find_or_add_connect_token_entry(&mut self, new_entry: ConnectTokenEntry) -> bool {
        let mut min = Duration::MAX;
        let mut oldest_entry = 0;
        let mut empty_entry = false;
        let mut matching_entry = None;
        for (i, entry) in self.connect_token_entries.iter().enumerate() {
            match entry {
                Some(e) => {
                    if e.mac == new_entry.mac {
                        matching_entry = Some(e);
                    }
                    if !empty_entry && e.time < min {
                        oldest_entry = i;
                        min = e.time;
                    }
                }
                None => {
                    if !empty_entry {
                        empty_entry = true;
                        oldest_entry = i;
                    }
                }
            }
        }

        if let Some(entry) = matching_entry {
            return entry.address == new_entry.address;
        }

        self.connect_token_entries[oldest_entry] = Some(new_entry);

        true
    }

    fn handle_connection_request<'a, 's>(
        &'s mut self,
        addr: SocketAddr,
        request: &ConnectionRequest,
    ) -> Result<ServerResult<'a, 's>, NetcodeError> {
        let connect_token = self.validate_client_token(request)?;

        let addr_already_connected = find_client_by_addr(&mut self.clients, addr).is_some();
        let id_already_connected = find_client_by_id(&mut self.clients, connect_token.client_id).is_some();
        if id_already_connected || addr_already_connected {
            // TODO(log): debug
            return Ok(ServerResult::None);
        }

        if !self.pending_clients.contains_key(&addr) && self.pending_clients.len() >= NETCODE_MAX_PENDING_CLIENTS {
            // TODO(log): debug
            return Ok(ServerResult::None);
        }

        let mut mac = [0u8; NETCODE_MAC_BYTES];
        mac.copy_from_slice(&request.data[NETCODE_CONNECT_TOKEN_PRIVATE_BYTES - NETCODE_MAC_BYTES..]);
        let connect_token_entry = ConnectTokenEntry {
            address: addr,
            time: self.current_time,
            mac,
        };

        if !self.find_or_add_connect_token_entry(connect_token_entry) {
            // TODO(log): debug
            return Ok(ServerResult::None);
        }

        if self.clients.iter().flatten().count() >= self.max_clients {
            self.pending_clients.remove(&addr);
            let packet = Packet::ConnectionDenied;
            let len = packet.encode(
                &mut self.out,
                self.protocol_id,
                Some((self.global_sequence, &connect_token.server_to_client_key)),
            )?;
            self.global_sequence += 1;
            return Ok(ServerResult::PacketToSend(&mut self.out[..len]));
        }

        self.challenge_sequence += 1;
        let packet = Packet::Challenge(EncryptedChallengeToken::generate(
            connect_token.client_id,
            &connect_token.user_data,
            self.challenge_sequence,
            &self.challenge_key,
        )?);

        let len = packet.encode(
            &mut self.out,
            self.protocol_id,
            Some((self.global_sequence, &connect_token.server_to_client_key)),
        )?;
        self.global_sequence += 1;

        let pending = self.pending_clients.entry(addr).or_insert_with(|| Connection {
            confirmed: false,
            sequence: 0,
            client_id: connect_token.client_id,
            last_packet_received_time: self.current_time,
            last_packet_send_time: self.current_time,
            addr,
            state: ConnectionState::PendingResponse,
            send_key: connect_token.server_to_client_key,
            receive_key: connect_token.client_to_server_key,
            timeout_seconds: connect_token.timeout_seconds,
            expire_timestamp: request.expire_timestamp,
            user_data: [0u8; NETCODE_USER_DATA_BYTES],
            replay_protection: ReplayProtection::new(),
        });
        pending.last_packet_received_time = self.current_time;
        pending.last_packet_send_time = self.current_time;

        Ok(ServerResult::PacketToSend(&mut self.out[..len]))
    }

    fn validate_client_token(&self, request: &ConnectionRequest) -> Result<PrivateConnectToken, NetcodeError> {
        if request.version_info != *NETCODE_VERSION_INFO {
            return Err(NetcodeError::InvalidVersion);
        }

        if request.protocol_id != self.protocol_id {
            return Err(NetcodeError::InvalidProtocolID);
        }

        if self.current_time.as_secs() >= request.expire_timestamp {
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

    pub fn generate_payload_packet<'s>(&'s mut self, slot: usize, payload: &[u8]) -> Result<&'s mut [u8], NetcodeError> {
        if slot >= self.clients.len() {
            return Err(NetcodeError::ClientNotFound);
        }

        if payload.len() > NETCODE_MAX_PAYLOAD_BYTES {
            return Err(NetcodeError::PayloadAboveLimit);
        }

        if let Some(client) = &mut self.clients[slot] {
            let packet = Packet::Payload(payload);
            let len = packet.encode(&mut self.out, self.protocol_id, Some((client.sequence, &client.send_key)))?;
            client.sequence += 1;
            return Ok(&mut self.out[..len]);
        }

        Err(NetcodeError::ClientNotFound)
    }

    pub fn process_packet<'a, 's>(&'s mut self, addr: SocketAddr, buffer: &'a mut [u8]) -> ServerResult<'a, 's> {
        match self.process_packet_internal(addr, buffer) {
            Err(_) => ServerResult::None,
            Ok(r) => r,
        }
    }

    fn process_packet_internal<'a, 's>(&'s mut self, addr: SocketAddr, buffer: &'a mut [u8]) -> Result<ServerResult<'a, 's>, NetcodeError> {
        if buffer.len() <= 2 + NETCODE_MAC_BYTES {
            return Ok(ServerResult::None);
        }

        // Handle connected client
        if let Some((slot, client)) = find_client_by_addr(&mut self.clients, addr) {
            let (_, packet) = Packet::decode(
                buffer,
                self.protocol_id,
                Some(&client.receive_key),
                Some(&mut client.replay_protection),
            )?;
            client.last_packet_received_time = self.current_time;
            match client.state {
                ConnectionState::Connected => match packet {
                    Packet::Disconnect => {
                        client.state = ConnectionState::Disconnected;
                        self.events.push_back(ServerEvent::ClientDisconnected(client.client_id));
                        self.clients[slot] = None;
                        // TODO(log): debug
                        return Ok(ServerResult::None);
                    }
                    Packet::Payload(payload) => {
                        if !client.confirmed {
                            // TODO(log): debug
                            client.confirmed = true;
                        }
                        return Ok(ServerResult::Payload(slot, payload));
                    }
                    Packet::KeepAlive(_) => {
                        if !client.confirmed {
                            // TODO(log): debug
                            client.confirmed = true;
                        }
                        return Ok(ServerResult::None);
                    }
                    _ => return Ok(ServerResult::None),
                },
                _ => return Ok(ServerResult::None),
            }
        }

        // Handle pending client
        if let Some(pending) = self.pending_clients.get_mut(&addr) {
            let (_, packet) = Packet::decode(
                buffer,
                self.protocol_id,
                Some(&pending.receive_key),
                Some(&mut pending.replay_protection),
            )?;
            pending.last_packet_received_time = self.current_time;
            match packet {
                Packet::ConnectionRequest(request) => return self.handle_connection_request(addr, &request),
                Packet::Response(response) => {
                    let challenge_token = response.decode(&self.challenge_key)?;
                    let mut pending = self.pending_clients.remove(&addr).unwrap();
                    if find_client_slot_by_id(&self.clients, challenge_token.client_id).is_some() {
                        // TODO(log): debug
                        return Ok(ServerResult::None);
                    }
                    match self.clients.iter().position(|c| c.is_none()) {
                        None => {
                            let packet = Packet::ConnectionDenied;
                            let len = packet.encode(&mut self.out, self.protocol_id, Some((self.global_sequence, &pending.send_key)))?;
                            pending.state = ConnectionState::Disconnected;
                            self.global_sequence += 1;
                            pending.last_packet_send_time = self.current_time;
                            return Ok(ServerResult::PacketToSend(&mut self.out[..len]));
                        }
                        Some(client_index) => {
                            self.events.push_back(ServerEvent::ClientConnected(pending.client_id));
                            let send_key = pending.send_key;
                            pending.state = ConnectionState::Connected;
                            pending.user_data = challenge_token.user_data;
                            pending.last_packet_send_time = self.current_time;
                            self.clients[client_index] = Some(pending);
                            let packet = Packet::KeepAlive(ConnectionKeepAlive {
                                max_clients: self.max_clients as u32,
                                client_index: client_index as u32,
                            });
                            let len = packet.encode(&mut self.out, self.protocol_id, Some((self.global_sequence, &send_key)))?;
                            self.global_sequence += 1;
                            return Ok(ServerResult::PacketToSend(&mut self.out[..len]));
                        }
                    }
                }
                _ => return Ok(ServerResult::None),
            }
        }

        // Handle new client
        let (_, packet) = Packet::decode(buffer, self.protocol_id, None, None)?;
        match packet {
            Packet::ConnectionRequest(request) => self.handle_connection_request(addr, &request),
            _ => Ok(ServerResult::None), // Decoding packet without key can only return ConnectionRequest
        }
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

    pub fn advance_time(&mut self, duration: Duration) {
        self.current_time += duration;
    }

    pub fn update_client(&mut self, slot: usize) -> Option<(&mut [u8], SocketAddr)> {
        if slot >= self.clients.len() {
            return None;
        }

        if let Some(client) = &mut self.clients[slot] {
            let connection_timed_out = client.timeout_seconds > 0
                && (client.last_packet_received_time + Duration::from_secs(client.timeout_seconds as u64) < self.current_time);
            if connection_timed_out {
                // TODO(log): debug
                client.state = ConnectionState::Disconnected;
            }

            if client.state == ConnectionState::Disconnected {
                self.events.push_back(ServerEvent::ClientDisconnected(client.client_id));
                let packet = Packet::Disconnect;
                let sequence = client.sequence;
                let send_key = client.send_key;
                let addr = client.addr;
                self.clients[slot] = None;
                let len = match packet.encode(&mut self.out, self.protocol_id, Some((sequence, &send_key))) {
                    Err(_) => return None,
                    Ok(len) => len,
                };
                return Some((&mut self.out[..len], addr));
            }

            if client.last_packet_send_time + NETCODE_SEND_RATE <= self.current_time {
                let packet = Packet::KeepAlive(ConnectionKeepAlive {
                    client_index: slot as u32,
                    max_clients: self.max_clients as u32,
                });
                let len = match packet.encode(&mut self.out, self.protocol_id, Some((client.sequence, &client.send_key))) {
                    Err(_) => return None,
                    Ok(len) => len,
                };
                client.sequence += 1;
                client.last_packet_send_time = self.current_time;
                return Some((&mut self.out[..len], client.addr));
            }
        }

        None
    }

    pub fn update_pending_connections(&mut self) {
        for client in self.pending_clients.values_mut() {
            if self.current_time.as_secs() > client.expire_timestamp {
                // TODO(log): debug
                client.state = ConnectionState::Disconnected;
            }
        }

        self.pending_clients.retain(|_, c| c.state != ConnectionState::Disconnected);
    }

    pub fn is_client_connected(&self, client_id: ClientID) -> bool {
        find_client_slot_by_id(&self.clients, client_id).is_some()
    }

    pub fn disconnect(&mut self, client_id: ClientID) -> Result<(&mut [u8], SocketAddr), NetcodeError> {
        if let Some(slot) = find_client_slot_by_id(&self.clients, client_id) {
            let client = self.clients[slot].take().unwrap();
            self.events.push_back(ServerEvent::ClientDisconnected(client_id));
            let packet = Packet::Disconnect;
            let len = packet.encode(&mut self.out, self.protocol_id, Some((client.sequence, &client.send_key)))?;
            return Ok((&mut self.out[..len], client.addr));
        }

        Err(NetcodeError::ClientNotFound)
    }
}

fn find_client_by_id(clients: &mut [Option<Connection>], client_id: ClientID) -> Option<(usize, &mut Connection)> {
    clients.iter_mut().enumerate().find_map(|(i, c)| match c {
        Some(c) if c.client_id == client_id => Some((i, c)),
        _ => None,
    })
}

fn find_client_slot_by_id(clients: &[Option<Connection>], client_id: ClientID) -> Option<usize> {
    clients.iter().enumerate().find_map(|(i, c)| match c {
        Some(c) if c.client_id == client_id => Some(i),
        _ => None,
    })
}

fn find_client_by_addr(clients: &mut [Option<Connection>], addr: SocketAddr) -> Option<(usize, &mut Connection)> {
    clients.iter_mut().enumerate().find_map(|(i, c)| match c {
        Some(c) if c.addr == addr => Some((i, c)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::{client::Client, token::ConnectToken};

    use super::*;

    #[test]
    fn server_connection() {
        let protocol_id = 7;
        let max_clients = 16;
        let server_addr = "127.0.0.1:5000".parse().unwrap();
        let private_key = b"an example very very secret key."; // 32-bytes
        let mut server = Server::new(Duration::ZERO, max_clients, protocol_id, server_addr, *private_key);

        let server_addresses: Vec<SocketAddr> = vec![server_addr];
        let user_data = generate_random_bytes();
        let expire_seconds = 3;
        let client_id = 4;
        let timeout_seconds = 5;
        let client_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let connect_token = ConnectToken::generate(
            Duration::ZERO,
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
        let client_packet = client.generate_packet().unwrap();

        let result = server.process_packet(client_addr, client_packet);
        assert!(matches!(result, ServerResult::PacketToSend(_)));
        match result {
            ServerResult::PacketToSend(packet) => client.process_packet(packet),
            _ => unreachable!(),
        };

        assert!(!client.connected());
        let client_packet = client.generate_packet().unwrap();
        let result = server.process_packet(client_addr, client_packet);
        assert!(matches!(result, ServerResult::PacketToSend(_)));

        match result {
            ServerResult::PacketToSend(packet) => client.process_packet(packet),
            _ => unreachable!(),
        };

        let client_connected = server.get_event().unwrap();
        assert_eq!(client_connected, ServerEvent::ClientConnected(client_id));

        assert!(client.connected());

        let payload = [7u8; 300];
        let result = server.generate_payload_packet(0, &payload).unwrap();
        let result_payload = client.process_packet(result).unwrap();
        assert_eq!(payload, result_payload);

        assert!(server.update_client(0).is_none());
        server.advance_time(NETCODE_SEND_RATE);

        let (keep_alive_packet, _) = server.update_client(0).unwrap();
        assert!(client.process_packet(keep_alive_packet).is_none());

        let client_payload = [2u8; 300];
        let result = client.generate_payload_packet(&client_payload).unwrap();

        match server.process_packet(client_addr, result) {
            ServerResult::Payload(slot, payload) => {
                assert_eq!(slot, 0);
                assert_eq!(client_payload, payload);
            }
            _ => unreachable!(),
        }

        assert!(server.is_client_connected(client_id));
        let (disconnect_packet, _) = server.disconnect(client_id).unwrap();

        assert!(client.connected());
        assert!(client.process_packet(disconnect_packet).is_none());

        assert!(!client.connected());
        assert!(!server.is_client_connected(client_id));

        let client_disconnected = server.get_event().unwrap();
        assert_eq!(client_disconnected, ServerEvent::ClientDisconnected(client_id));
    }

    #[test]
    fn connect_token_already_used() {
        let server_addr = "127.0.0.1:5000".parse().unwrap();
        let private_key = b"an example very very secret key."; // 32-bytes
        let mut server = Server::new(Duration::ZERO, 16, 0, server_addr, *private_key);

        let client_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let mut connect_token = ConnectTokenEntry {
            time: Duration::ZERO,
            address: client_addr,
            mac: generate_random_bytes(),
        };
        // Allow first entry
        assert!(server.find_or_add_connect_token_entry(connect_token));
        // Allow same token with the same address
        assert!(server.find_or_add_connect_token_entry(connect_token));
        connect_token.address = "127.0.0.1:3001".parse().unwrap();

        // Don't allow same token with different address
        assert!(!server.find_or_add_connect_token_entry(connect_token));
    }
}
