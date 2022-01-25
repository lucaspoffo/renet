use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    packet::{ConnectionChallenge, ConnectionRequest, ConnectionResponse, NetcodeError, Packet},
    token::PrivateConnectToken,
    NETCODE_KEY_BYTES, NETCODE_VERSION_INFO,
};

enum ConnectionState {
    Disconnected,
    PendingResponse,
    TimedOut,
    Connected,
}

struct Connection {
    state: ConnectionState,
    send_key: [u8; NETCODE_KEY_BYTES],
    receive_key: [u8; NETCODE_KEY_BYTES],
    addr: SocketAddr,
}

struct Server {
    clients: HashMap<u64, Connection>,
    protocol_id: u64,
    connect_key: [u8; NETCODE_KEY_BYTES],
    max_clients: usize,
    challenge_sequence: u64,
    challenge_key: [u8; NETCODE_KEY_BYTES],
    address: SocketAddr,
}

impl Server {
    pub fn handle_connection_request(
        &mut self,
        addr: SocketAddr,
        mut request: ConnectionRequest,
    ) -> Result<Option<Packet<'_>>, NetcodeError> {
        let connect_token = self.validate_client_token(&mut request)?;
        if !self.clients.contains_key(&connect_token.client_id) && self.clients.len() >= self.max_clients {
            return Ok(Some(Packet::ConnectionDenied));
        }

        self.clients.entry(connect_token.client_id).or_insert_with(|| Connection {
            addr,
            state: ConnectionState::PendingResponse,
            send_key: connect_token.server_to_client_key,
            receive_key: connect_token.client_to_server_key,
        });

        self.challenge_sequence += 1;
        let packet = Packet::Challenge(ConnectionChallenge::generate(
            connect_token.client_id,
            &connect_token.user_data,
            self.challenge_sequence,
            &self.challenge_key,
        )?);

        Ok(Some(packet))
    }

    // TODO(error): should we return Result?
    pub fn validate_client_token(&self, request: &mut ConnectionRequest) -> Result<PrivateConnectToken, NetcodeError> {
        if request.version_info != *NETCODE_VERSION_INFO {
            return Err(NetcodeError::InvalidVersion);
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if now > request.expire_timestamp {
            return Err(NetcodeError::Expired);
        }

        let token = PrivateConnectToken::decode(
            &mut request.data,
            self.protocol_id,
            request.expire_timestamp,
            request.sequence,
            &self.connect_key,
        )?;

        let in_host_list = token.server_addresses.iter().any(|host| *host == Some(self.address));
        if in_host_list {
            Ok(token)
        } else {
            Err(NetcodeError::NotInHostList)
        }
    }

    fn process_packet(&mut self, addr: SocketAddr, packet: Packet) -> Option<Packet<'_>> {
        let result = self.clients.iter().find(|(id, connection)| connection.addr == addr);
        match result {
            Some((client_id, connection)) => match connection.state {
                _ => None,
            },
            None => match packet {
                Packet::ConnectionRequest(request) => match self.handle_connection_request(addr, request) {
                    Ok(p) => p,
                    Err(_) => None
                }
                _ => {
                    // TODO(log): log expected connection request
                    None
                }
            },
        }
    }
}
