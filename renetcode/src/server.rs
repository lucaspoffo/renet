use std::{collections::HashMap, net::SocketAddr, time::Duration};

use crate::{
    crypto::generate_random_bytes,
    packet::{ChallengeToken, Packet},
    replay_protection::ReplayProtection,
    token::PrivateConnectToken,
    ClientID, NetcodeError, NETCODE_CONNECT_TOKEN_PRIVATE_BYTES, NETCODE_CONNECT_TOKEN_XNONCE_BYTES, NETCODE_KEY_BYTES, NETCODE_MAC_BYTES,
    NETCODE_MAX_CLIENTS, NETCODE_MAX_PACKET_BYTES, NETCODE_MAX_PAYLOAD_BYTES, NETCODE_MAX_PENDING_CLIENTS, NETCODE_SEND_RATE,
    NETCODE_USER_DATA_BYTES, NETCODE_VERSION_INFO,
};

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

/// A server that can generate packets from connect clients, that are encrypted, or process
/// incoming encrypted packets from clients. The server is agnostic from the transport layer, only
/// consuming and generating bytes that can be transported in any way desired.
#[derive(Debug)]
pub struct NetcodeServer {
    clients: Box<[Option<Connection>]>,
    pending_clients: HashMap<SocketAddr, Connection>,
    connect_token_entries: Box<[Option<ConnectTokenEntry>; NETCODE_MAX_CLIENTS * 2]>,
    protocol_id: u64,
    connect_key: [u8; NETCODE_KEY_BYTES],
    max_clients: usize,
    challenge_sequence: u64,
    challenge_key: [u8; NETCODE_KEY_BYTES],
    public_address: SocketAddr,
    current_time: Duration,
    global_sequence: u64,
    out: [u8; NETCODE_MAX_PACKET_BYTES],
}

/// Result from processing an packet in the server
#[derive(Debug, PartialEq, Eq)]
pub enum ServerResult<'a, 's> {
    /// Nothing needs to be done.
    None,
    /// A packet to be sent back to the processed address.
    PacketToSend { addr: SocketAddr, payload: &'s mut [u8] },
    /// A payload received from the client.
    Payload { client_id: ClientID, payload: &'a [u8] },
    /// A new client has connected
    ClientConnected {
        client_id: ClientID,
        addr: SocketAddr,
        user_data: Box<[u8; NETCODE_USER_DATA_BYTES]>,
        payload: &'s mut [u8],
    },
    /// The client connection has been terminated.
    ClientDisconnected {
        client_id: ClientID,
        addr: SocketAddr,
        payload: Option<&'s mut [u8]>,
    },
}

impl NetcodeServer {
    /// Starts a new NetcodeServer.
    ///
    /// # Arguments
    ///
    /// * `public_address` - publicly available address to which clients will attempt to connect. This is
    /// the address used to generate the ConnectToken.
    ///
    /// * `protocol_id` - unique identifier for this particular game/application.
    /// You can use a hash function with the current version of the game to generate this value
    /// so that older versions cannot connect to newer versions.
    pub fn new(
        current_time: Duration,
        max_clients: usize,
        protocol_id: u64,
        public_address: SocketAddr,
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
            connect_token_entries: Box::new([None; NETCODE_MAX_CLIENTS * 2]),
            pending_clients: HashMap::new(),
            protocol_id,
            connect_key: private_key,
            max_clients,
            challenge_sequence: 0,
            global_sequence: 0,
            challenge_key,
            public_address,
            current_time,
            out: [0u8; NETCODE_MAX_PACKET_BYTES],
        }
    }

    #[doc(hidden)]
    pub fn __test() -> Self {
        Self::new(Duration::ZERO, 32, 0, "127.0.0.1:0".parse().unwrap(), [0u8; NETCODE_KEY_BYTES])
    }

    pub fn address(&self) -> SocketAddr {
        self.public_address
    }

    pub fn current_time(&self) -> Duration {
        self.current_time
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

    /// Returns the user data from the connected client.
    pub fn user_data(&self, client_id: ClientID) -> Option<[u8; NETCODE_USER_DATA_BYTES]> {
        if let Some(client) = find_client_by_id(&self.clients, client_id) {
            return Some(client.user_data);
        }

        None
    }

    /// Returns the duration since the connected client last received a packet.
    /// Usefull to detect users that are timing out.
    pub fn time_since_last_received_packet(&self, client_id: ClientID) -> Option<Duration> {
        if let Some(client) = find_client_by_id(&self.clients, client_id) {
            let time = self.current_time - client.last_packet_received_time;
            return Some(time);
        }

        None
    }

    /// Returns the client address if connected.
    pub fn client_addr(&self, client_id: ClientID) -> Option<SocketAddr> {
        if let Some(client) = find_client_by_id(&self.clients, client_id) {
            return Some(client.addr);
        }

        None
    }

    fn handle_connection_request<'a>(
        &mut self,
        addr: SocketAddr,
        version_info: [u8; 13],
        protocol_id: u64,
        expire_timestamp: u64,
        xnonce: [u8; NETCODE_CONNECT_TOKEN_XNONCE_BYTES],
        data: [u8; NETCODE_CONNECT_TOKEN_PRIVATE_BYTES],
    ) -> Result<ServerResult<'a, '_>, NetcodeError> {
        if version_info != *NETCODE_VERSION_INFO {
            return Err(NetcodeError::InvalidVersion);
        }

        if protocol_id != self.protocol_id {
            return Err(NetcodeError::InvalidProtocolID);
        }

        if self.current_time.as_secs() >= expire_timestamp {
            return Err(NetcodeError::Expired);
        }

        let connect_token = PrivateConnectToken::decode(&data, self.protocol_id, expire_timestamp, &xnonce, &self.connect_key)?;

        let in_host_list = connect_token.server_addresses.iter().any(|host| *host == Some(self.public_address));
        if !in_host_list {
            return Err(NetcodeError::NotInHostList);
        }

        let addr_already_connected = find_client_mut_by_addr(&mut self.clients, addr).is_some();
        let id_already_connected = find_client_mut_by_id(&mut self.clients, connect_token.client_id).is_some();
        if id_already_connected || addr_already_connected {
            log::debug!(
                "Connection request denied: client {} already connected (address: {}).",
                connect_token.client_id,
                addr
            );
            return Ok(ServerResult::None);
        }

        if !self.pending_clients.contains_key(&addr) && self.pending_clients.len() >= NETCODE_MAX_PENDING_CLIENTS {
            log::warn!(
                "Connection request denied: reached max amount allowed of pending clients ({}).",
                NETCODE_MAX_PENDING_CLIENTS
            );
            return Ok(ServerResult::None);
        }

        let mut mac = [0u8; NETCODE_MAC_BYTES];
        mac.copy_from_slice(&data[NETCODE_CONNECT_TOKEN_PRIVATE_BYTES - NETCODE_MAC_BYTES..]);
        let connect_token_entry = ConnectTokenEntry {
            address: addr,
            time: self.current_time,
            mac,
        };

        if !self.find_or_add_connect_token_entry(connect_token_entry) {
            log::warn!("Connection request denied: unable to add connect token entry");
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
            return Ok(ServerResult::PacketToSend {
                addr,
                payload: &mut self.out[..len],
            });
        }

        self.challenge_sequence += 1;
        let packet = Packet::generate_challenge(
            connect_token.client_id,
            &connect_token.user_data,
            self.challenge_sequence,
            &self.challenge_key,
        )?;

        let len = packet.encode(
            &mut self.out,
            self.protocol_id,
            Some((self.global_sequence, &connect_token.server_to_client_key)),
        )?;
        self.global_sequence += 1;

        log::trace!("Connection request from Client {}", connect_token.client_id);

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
            expire_timestamp,
            user_data: connect_token.user_data,
            replay_protection: ReplayProtection::new(),
        });
        pending.last_packet_received_time = self.current_time;
        pending.last_packet_send_time = self.current_time;

        Ok(ServerResult::PacketToSend {
            addr,
            payload: &mut self.out[..len],
        })
    }

    /// Returns an encoded packet payload to be sent to the client
    pub fn generate_payload_packet<'s>(
        &'s mut self,
        client_id: ClientID,
        payload: &[u8],
    ) -> Result<(SocketAddr, &'s mut [u8]), NetcodeError> {
        if payload.len() > NETCODE_MAX_PAYLOAD_BYTES {
            return Err(NetcodeError::PayloadAboveLimit);
        }

        if let Some(client) = find_client_mut_by_id(&mut self.clients, client_id) {
            let packet = Packet::Payload(payload);
            let len = packet.encode(&mut self.out, self.protocol_id, Some((client.sequence, &client.send_key)))?;
            client.sequence += 1;
            client.last_packet_send_time = self.current_time;

            return Ok((client.addr, &mut self.out[..len]));
        }

        Err(NetcodeError::ClientNotFound)
    }

    /// Process an packet from the especifed address. Returns a server result, check out
    /// [ServerResult].
    pub fn process_packet<'a, 's>(&'s mut self, addr: SocketAddr, buffer: &'a mut [u8]) -> ServerResult<'a, 's> {
        match self.process_packet_internal(addr, buffer) {
            Err(e) => {
                log::error!("Failed to process packet: {}", e);
                ServerResult::None
            }
            Ok(r) => r,
        }
    }

    fn process_packet_internal<'a, 's>(&'s mut self, addr: SocketAddr, buffer: &'a mut [u8]) -> Result<ServerResult<'a, 's>, NetcodeError> {
        if buffer.len() < 2 + NETCODE_MAC_BYTES {
            return Err(NetcodeError::PacketTooSmall);
        }

        // Handle connected client
        if let Some((slot, client)) = find_client_mut_by_addr(&mut self.clients, addr) {
            let (_, packet) = Packet::decode(
                buffer,
                self.protocol_id,
                Some(&client.receive_key),
                Some(&mut client.replay_protection),
            )?;
            log::trace!(
                "Received packet from connected client ({}): {:?}",
                client.client_id,
                packet.packet_type()
            );

            client.last_packet_received_time = self.current_time;
            match client.state {
                ConnectionState::Connected => match packet {
                    Packet::Disconnect => {
                        client.state = ConnectionState::Disconnected;
                        let client_id = client.client_id;
                        self.clients[slot] = None;
                        log::trace!("Client {} requested to disconnect", client_id);
                        return Ok(ServerResult::ClientDisconnected {
                            client_id,
                            addr,
                            payload: None,
                        });
                    }
                    Packet::Payload(payload) => {
                        if !client.confirmed {
                            log::trace!("Confirmed connection for Client {}", client.client_id);
                            client.confirmed = true;
                        }
                        return Ok(ServerResult::Payload {
                            client_id: client.client_id,
                            payload,
                        });
                    }
                    Packet::KeepAlive { .. } => {
                        if !client.confirmed {
                            log::trace!("Confirmed connection for Client {}", client.client_id);
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
            log::trace!("Received packet from pending client ({}): {:?}", addr, packet.packet_type());
            match packet {
                Packet::ConnectionRequest {
                    protocol_id,
                    expire_timestamp,
                    data,
                    xnonce,
                    version_info,
                } => {
                    return self.handle_connection_request(addr, version_info, protocol_id, expire_timestamp, xnonce, data);
                }
                Packet::Response {
                    token_data,
                    token_sequence,
                } => {
                    let challenge_token = ChallengeToken::decode(token_data, token_sequence, &self.challenge_key)?;
                    let mut pending = self.pending_clients.remove(&addr).unwrap();
                    if find_client_slot_by_id(&self.clients, challenge_token.client_id).is_some() {
                        log::debug!(
                            "Ignored connection response for Client {}, already connected.",
                            challenge_token.client_id
                        );
                        return Ok(ServerResult::None);
                    }
                    match self.clients.iter().position(|c| c.is_none()) {
                        None => {
                            let packet = Packet::ConnectionDenied;
                            let len = packet.encode(&mut self.out, self.protocol_id, Some((self.global_sequence, &pending.send_key)))?;
                            pending.state = ConnectionState::Disconnected;
                            self.global_sequence += 1;
                            pending.last_packet_send_time = self.current_time;
                            return Ok(ServerResult::PacketToSend {
                                addr,
                                payload: &mut self.out[..len],
                            });
                        }
                        Some(client_index) => {
                            pending.state = ConnectionState::Connected;
                            pending.user_data = challenge_token.user_data;
                            pending.last_packet_send_time = self.current_time;

                            let packet = Packet::KeepAlive {
                                max_clients: self.max_clients as u32,
                                client_index: client_index as u32,
                            };
                            let len = packet.encode(&mut self.out, self.protocol_id, Some((pending.sequence, &pending.send_key)))?;
                            pending.sequence += 1;

                            let client_id: ClientID = pending.client_id;
                            let user_data: [u8; NETCODE_USER_DATA_BYTES] = pending.user_data;
                            self.clients[client_index] = Some(pending);

                            return Ok(ServerResult::ClientConnected {
                                client_id,
                                addr,
                                user_data: Box::new(user_data),
                                payload: &mut self.out[..len],
                            });
                        }
                    }
                }
                _ => return Ok(ServerResult::None),
            }
        }

        // Handle new client
        let (_, packet) = Packet::decode(buffer, self.protocol_id, None, None)?;
        match packet {
            Packet::ConnectionRequest {
                data,
                protocol_id,
                expire_timestamp,
                xnonce,
                version_info,
            } => self.handle_connection_request(addr, version_info, protocol_id, expire_timestamp, xnonce, data),
            _ => unreachable!("Decoding packet without key can only return ConnectionRequest packets"),
        }
    }

    pub fn clients_slot(&self) -> Vec<usize> {
        self.clients
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| if slot.is_some() { Some(index) } else { None })
            .collect()
    }

    /// Returns the ids from the connected clients (iterator).
    pub fn clients_id_iter(&self) -> impl Iterator<Item = ClientID> + '_ {
        self.clients.iter().filter_map(|slot| slot.as_ref().map(|client| client.client_id))
    }

    /// Returns the ids from the connected clients.
    pub fn clients_id(&self) -> Vec<ClientID> {
        self.clients_id_iter().collect()
    }

    /// Returns the maximum number of clients that can be connected.
    pub fn max_clients(&self) -> usize {
        self.max_clients
    }

    /// Returns current number of clients connected.
    pub fn connected_clients(&self) -> usize {
        self.clients.iter().filter(|slot| slot.is_some()).count()
    }

    /// Advance the server current time, and remove any pending connections that have expired.
    pub fn update(&mut self, duration: Duration) {
        self.current_time += duration;

        for client in self.pending_clients.values_mut() {
            if self.current_time.as_secs() > client.expire_timestamp {
                log::debug!("Pending Client {} disconnected, connection token expired.", client.client_id);
                client.state = ConnectionState::Disconnected;
            }
        }

        self.pending_clients.retain(|_, c| c.state != ConnectionState::Disconnected);
    }

    /// Updates the client, returns a ServerResult.
    ///
    /// # Example
    /// ```
    /// # use renetcode::ServerResult;
    /// # let mut server = renetcode::NetcodeServer::__test();
    /// for client_id in server.clients_id().into_iter() {
    ///     match server.update_client(client_id) {
    ///         ServerResult::PacketToSend { payload, addr } => send_to(payload, addr),
    ///         _ => { /* ... */ }
    ///     }
    /// }
    /// # fn send_to(p: &[u8], addr: std::net::SocketAddr) {}
    /// ```
    pub fn update_client(&mut self, client_id: ClientID) -> ServerResult<'_, '_> {
        let slot = match find_client_slot_by_id(&self.clients, client_id) {
            None => return ServerResult::None,
            Some(slot) => slot,
        };

        if let Some(client) = &mut self.clients[slot] {
            let connection_timed_out = client.timeout_seconds > 0
                && (client.last_packet_received_time + Duration::from_secs(client.timeout_seconds as u64) < self.current_time);
            if connection_timed_out {
                log::debug!("Client {} disconnected, connection timed out", client.client_id);
                client.state = ConnectionState::Disconnected;
            }

            if client.state == ConnectionState::Disconnected {
                let packet = Packet::Disconnect;
                let sequence = client.sequence;
                let send_key = client.send_key;
                let addr = client.addr;
                self.clients[slot] = None;

                let len = match packet.encode(&mut self.out, self.protocol_id, Some((sequence, &send_key))) {
                    Err(e) => {
                        log::error!("Failed to encode disconnect packet: {}", e);
                        return ServerResult::ClientDisconnected {
                            client_id,
                            addr,
                            payload: None,
                        };
                    }
                    Ok(len) => len,
                };

                return ServerResult::ClientDisconnected {
                    client_id,
                    addr,
                    payload: Some(&mut self.out[..len]),
                };
            }

            if client.last_packet_send_time + NETCODE_SEND_RATE <= self.current_time {
                let packet = Packet::KeepAlive {
                    client_index: slot as u32,
                    max_clients: self.max_clients as u32,
                };

                let len = match packet.encode(&mut self.out, self.protocol_id, Some((client.sequence, &client.send_key))) {
                    Err(e) => {
                        log::error!("Failed to encode keep alive packet: {}", e);
                        return ServerResult::None;
                    }
                    Ok(len) => len,
                };
                client.sequence += 1;
                client.last_packet_send_time = self.current_time;
                return ServerResult::PacketToSend {
                    addr: client.addr,
                    payload: &mut self.out[..len],
                };
            }
        }

        ServerResult::None
    }

    pub fn is_client_connected(&self, client_id: ClientID) -> bool {
        find_client_slot_by_id(&self.clients, client_id).is_some()
    }

    /// Disconnect an client and returns its address and a disconnect packet to be sent to them.
    // TODO: we can return Result<PacketToSend, NetcodeError>
    //       but the library user would need to be aware that he has to run
    //       the same code as Result::ClientDisconnected
    pub fn disconnect(&mut self, client_id: ClientID) -> ServerResult<'_, '_> {
        if let Some(slot) = find_client_slot_by_id(&self.clients, client_id) {
            let client = self.clients[slot].take().unwrap();
            let packet = Packet::Disconnect;

            let len = match packet.encode(&mut self.out, self.protocol_id, Some((client.sequence, &client.send_key))) {
                Err(e) => {
                    log::error!("Failed to encode disconnect packet: {}", e);
                    return ServerResult::ClientDisconnected {
                        client_id,
                        addr: client.addr,
                        payload: None,
                    };
                }
                Ok(len) => len,
            };
            return ServerResult::ClientDisconnected {
                client_id,
                addr: client.addr,
                payload: Some(&mut self.out[..len]),
            };
        }

        ServerResult::None
    }
}

fn find_client_mut_by_id(clients: &mut [Option<Connection>], client_id: ClientID) -> Option<&mut Connection> {
    clients.iter_mut().flatten().find(|c| c.client_id == client_id)
}

fn find_client_by_id(clients: &[Option<Connection>], client_id: ClientID) -> Option<&Connection> {
    clients.iter().flatten().find(|c| c.client_id == client_id)
}

fn find_client_slot_by_id(clients: &[Option<Connection>], client_id: ClientID) -> Option<usize> {
    clients.iter().enumerate().find_map(|(i, c)| match c {
        Some(c) if c.client_id == client_id => Some(i),
        _ => None,
    })
}

fn find_client_mut_by_addr(clients: &mut [Option<Connection>], addr: SocketAddr) -> Option<(usize, &mut Connection)> {
    clients.iter_mut().enumerate().find_map(|(i, c)| match c {
        Some(c) if c.addr == addr => Some((i, c)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::{client::NetcodeClient, token::ConnectToken};

    use super::*;

    const TEST_KEY: &[u8; NETCODE_KEY_BYTES] = b"an example very very secret key."; // 32-bytes
    const TEST_PROTOCOL_ID: u64 = 7;

    fn new_server() -> NetcodeServer {
        let max_clients = 16;
        let server_addr = "127.0.0.1:5000".parse().unwrap();
        NetcodeServer::new(Duration::ZERO, max_clients, TEST_PROTOCOL_ID, server_addr, *TEST_KEY)
    }

    #[test]
    fn server_connection() {
        let mut server = new_server();
        let server_addresses: Vec<SocketAddr> = vec![server.address()];
        let user_data = generate_random_bytes();
        let expire_seconds = 3;
        let client_id = 4;
        let timeout_seconds = 5;
        let client_addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let connect_token = ConnectToken::generate(
            Duration::ZERO,
            TEST_PROTOCOL_ID,
            expire_seconds,
            client_id,
            timeout_seconds,
            server_addresses,
            Some(&user_data),
            TEST_KEY,
        )
        .unwrap();
        let mut client = NetcodeClient::new(Duration::ZERO, connect_token);
        let (client_packet, _) = client.update(Duration::ZERO).unwrap();

        let result = server.process_packet(client_addr, client_packet);
        assert!(matches!(result, ServerResult::PacketToSend { .. }));
        match result {
            ServerResult::PacketToSend { payload, .. } => client.process_packet(payload),
            _ => unreachable!(),
        };

        assert!(!client.is_connected());
        let (client_packet, _) = client.update(Duration::ZERO).unwrap();
        let result = server.process_packet(client_addr, client_packet);

        match result {
            ServerResult::ClientConnected {
                client_id: r_id,
                user_data: r_data,
                payload,
                ..
            } => {
                assert_eq!(client_id, r_id);
                assert_eq!(user_data, *r_data);
                client.process_packet(payload)
            }
            _ => unreachable!(),
        };

        assert!(client.is_connected());

        for _ in 0..3 {
            let payload = [7u8; 300];
            let (_, packet) = server.generate_payload_packet(client_id, &payload).unwrap();
            let result_payload = client.process_packet(packet).unwrap();
            assert_eq!(payload, result_payload);
        }

        let result = server.update_client(client_id);
        assert_eq!(result, ServerResult::None);
        server.update(NETCODE_SEND_RATE);

        let result = server.update_client(client_id);
        match result {
            ServerResult::PacketToSend { payload, .. } => {
                assert!(client.process_packet(payload).is_none());
            }
            _ => unreachable!(),
        }

        let client_payload = [2u8; 300];
        let (_, packet) = client.generate_payload_packet(&client_payload).unwrap();

        match server.process_packet(client_addr, packet) {
            ServerResult::Payload { client_id: id, payload } => {
                assert_eq!(id, client_id);
                assert_eq!(client_payload, payload);
            }
            _ => unreachable!(),
        }

        assert!(server.is_client_connected(client_id));
        let result = server.disconnect(client_id);
        match result {
            ServerResult::ClientDisconnected {
                payload: Some(payload), ..
            } => {
                assert!(client.is_connected());
                assert!(client.process_packet(payload).is_none());
                assert!(!client.is_connected());
            }
            _ => unreachable!(),
        }

        assert!(!server.is_client_connected(client_id));
    }

    #[test]
    fn connect_token_already_used() {
        let mut server = new_server();

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
