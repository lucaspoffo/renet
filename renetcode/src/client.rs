use std::{fmt, net::SocketAddr, time::Duration};

use crate::{
    packet::Packet, replay_protection::ReplayProtection, token::ConnectToken, ClientID, NetcodeError, NETCODE_CHALLENGE_TOKEN_BYTES,
    NETCODE_MAX_PACKET_BYTES, NETCODE_MAX_PAYLOAD_BYTES, NETCODE_SEND_RATE,
};

/// The reason why a client is in error state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectReason {
    ConnectTokenExpired,
    ConnectionTimedOut,
    ConnectionResponseTimedOut,
    ConnectionRequestTimedOut,
    ConnectionDenied,
    DisconnectedByClient,
    DisconnectedByServer,
}

#[derive(Debug, PartialEq, Eq)]
enum ClientState {
    Disconnected(DisconnectReason),
    SendingConnectionRequest,
    SendingConnectionResponse,
    Connected,
}

/// A client that can generate encrypted packets that be sent to the connected server, or consume
/// encrypted packets from the server.
/// The client is agnostic from the transport layer, only consuming and generating bytes
/// that can be transported in any way desired.
#[derive(Debug)]
pub struct NetcodeClient {
    state: ClientState,
    client_id: ClientID,
    connect_start_time: Duration,
    last_packet_send_time: Option<Duration>,
    last_packet_received_time: Duration,
    current_time: Duration,
    sequence: u64,
    server_addr: SocketAddr,
    server_addr_index: usize,
    connect_token: ConnectToken,
    challenge_token_sequence: u64,
    challenge_token_data: [u8; NETCODE_CHALLENGE_TOKEN_BYTES],
    max_clients: u32,
    client_index: u32,
    send_rate: Duration,
    replay_protection: ReplayProtection,
    out: [u8; NETCODE_MAX_PACKET_BYTES],
}

impl fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DisconnectReason::*;

        match *self {
            ConnectTokenExpired => write!(f, "connection token has expired"),
            ConnectionTimedOut => write!(f, "connection timed out"),
            ConnectionResponseTimedOut => write!(f, "connection timed out during response step"),
            ConnectionRequestTimedOut => write!(f, "connection timed out during request step"),
            ConnectionDenied => write!(f, "server denied connection"),
            DisconnectedByClient => write!(f, "connection terminated by client"),
            DisconnectedByServer => write!(f, "connection terminated by server"),
        }
    }
}

impl NetcodeClient {
    pub fn new(current_time: Duration, connect_token: ConnectToken) -> Self {
        let server_addr = connect_token.server_addresses[0].expect("cannot create or deserialize a ConnectToken without a server address");

        Self {
            sequence: 0,
            client_id: connect_token.client_id,
            server_addr,
            server_addr_index: 0,
            challenge_token_sequence: 0,
            state: ClientState::SendingConnectionRequest,
            connect_start_time: current_time,
            last_packet_send_time: None,
            last_packet_received_time: current_time,
            current_time,
            max_clients: 0,
            client_index: 0,
            send_rate: NETCODE_SEND_RATE,
            challenge_token_data: [0u8; NETCODE_CHALLENGE_TOKEN_BYTES],
            connect_token,
            replay_protection: ReplayProtection::new(),
            out: [0u8; NETCODE_MAX_PACKET_BYTES],
        }
    }

    pub fn is_connecting(&self) -> bool {
        matches!(
            self.state,
            ClientState::SendingConnectionRequest | ClientState::SendingConnectionResponse
        )
    }

    pub fn is_connected(&self) -> bool {
        self.state == ClientState::Connected
    }

    pub fn is_disconnected(&self) -> bool {
        matches!(self.state, ClientState::Disconnected(_))
    }

    pub fn current_time(&self) -> Duration {
        self.current_time
    }

    pub fn client_id(&self) -> ClientID {
        self.client_id
    }

    /// Returns the duration since the client last received a packet.
    /// Usefull to detect timeouts.
    pub fn time_since_last_received_packet(&self) -> Duration {
        self.current_time - self.last_packet_received_time
    }

    /// Returns the reason that the client was disconnected for.
    pub fn disconnect_reason(&self) -> Option<DisconnectReason> {
        if let ClientState::Disconnected(reason) = &self.state {
            return Some(*reason);
        }
        None
    }

    /// Returns the current server address the client is connected or trying to connect.
    pub fn server_addr(&self) -> SocketAddr {
        self.server_addr
    }

    /// Disconnect the client from the server.
    /// Returns a disconnect packet that should be sent to the server.
    pub fn disconnect(&mut self) -> Result<(SocketAddr, &mut [u8]), NetcodeError> {
        self.state = ClientState::Disconnected(DisconnectReason::DisconnectedByClient);
        let packet = Packet::Disconnect;
        let len = packet.encode(
            &mut self.out,
            self.connect_token.protocol_id,
            Some((self.sequence, &self.connect_token.client_to_server_key)),
        )?;

        Ok((self.server_addr, &mut self.out[..len]))
    }

    /// Process any packet received from the server. This function might return a payload sent from the
    /// server. If nothing is returned, it was a packet used for the internal protocol or an
    /// invalid packet.
    pub fn process_packet<'a>(&mut self, buffer: &'a mut [u8]) -> Option<&'a [u8]> {
        let packet = match Packet::decode(
            buffer,
            self.connect_token.protocol_id,
            Some(&self.connect_token.server_to_client_key),
            Some(&mut self.replay_protection),
        ) {
            Ok((_, packet)) => packet,
            Err(e) => {
                log::error!("Failed to decode packet: {}", e);
                return None;
            }
        };
        log::trace!("Received packet from server: {:?}", packet.packet_type());

        match (packet, &self.state) {
            (Packet::ConnectionDenied, ClientState::SendingConnectionRequest | ClientState::SendingConnectionResponse) => {
                self.state = ClientState::Disconnected(DisconnectReason::ConnectionDenied);
                self.last_packet_received_time = self.current_time;
            }
            (
                Packet::Challenge {
                    token_data,
                    token_sequence,
                },
                ClientState::SendingConnectionRequest,
            ) => {
                self.challenge_token_sequence = token_sequence;
                self.last_packet_received_time = self.current_time;
                self.last_packet_send_time = None;
                self.challenge_token_data = token_data;
                self.state = ClientState::SendingConnectionResponse;
            }
            (Packet::KeepAlive { .. }, ClientState::Connected) => {
                self.last_packet_received_time = self.current_time;
            }
            (Packet::KeepAlive { client_index, max_clients }, ClientState::SendingConnectionResponse) => {
                self.last_packet_received_time = self.current_time;
                self.max_clients = max_clients;
                self.client_index = client_index;
                self.state = ClientState::Connected;
            }
            (Packet::Payload(p), ClientState::Connected) => {
                self.last_packet_received_time = self.current_time;
                return Some(p);
            }
            (Packet::Disconnect, ClientState::Connected) => {
                self.state = ClientState::Disconnected(DisconnectReason::DisconnectedByServer);
                self.last_packet_received_time = self.current_time;
            }
            _ => {}
        }

        None
    }

    /// Returns the server address and an encrypted payload packet that can be sent to the server.
    pub fn generate_payload_packet(&mut self, payload: &[u8]) -> Result<(SocketAddr, &mut [u8]), NetcodeError> {
        if payload.len() > NETCODE_MAX_PAYLOAD_BYTES {
            return Err(NetcodeError::PayloadAboveLimit);
        }

        if self.state != ClientState::Connected {
            return Err(NetcodeError::ClientNotConnected);
        }

        let packet = Packet::Payload(payload);
        let len = packet.encode(
            &mut self.out,
            self.connect_token.protocol_id,
            Some((self.sequence, &self.connect_token.client_to_server_key)),
        )?;
        self.sequence += 1;
        self.last_packet_send_time = Some(self.current_time);

        Ok((self.server_addr, &mut self.out[..len]))
    }

    /// Update the internal state of the client, receives the duration since last updated.
    /// Might return the serve address and a protocol packet to be sent to the server.
    pub fn update(&mut self, duration: Duration) -> Option<(&mut [u8], SocketAddr)> {
        if let Err(e) = self.update_internal_state(duration) {
            log::error!("Failed to update client: {}", e);
            return None;
        }

        // Generate packet for the current state
        self.generate_packet()
    }

    fn update_internal_state(&mut self, duration: Duration) -> Result<(), NetcodeError> {
        self.current_time += duration;
        let connection_timed_out = self.connect_token.timeout_seconds > 0
            && (self.last_packet_received_time + Duration::from_secs(self.connect_token.timeout_seconds as u64) < self.current_time);

        match self.state {
            ClientState::SendingConnectionRequest | ClientState::SendingConnectionResponse => {
                let expire_seconds = self.connect_token.expire_timestamp - self.connect_token.create_timestamp;
                let connection_expired = (self.current_time - self.connect_start_time).as_secs() >= expire_seconds;
                if connection_expired {
                    self.state = ClientState::Disconnected(DisconnectReason::ConnectTokenExpired);
                    return Err(NetcodeError::Expired);
                }
                if connection_timed_out {
                    let reason = if self.state == ClientState::SendingConnectionResponse {
                        DisconnectReason::ConnectionResponseTimedOut
                    } else {
                        DisconnectReason::ConnectionRequestTimedOut
                    };
                    self.state = ClientState::Disconnected(reason);
                    // Try to connect to the next server address
                    self.server_addr_index += 1;
                    if self.server_addr_index >= 32 {
                        return Err(NetcodeError::NoMoreServers);
                    }
                    match self.connect_token.server_addresses[self.server_addr_index] {
                        None => return Err(NetcodeError::NoMoreServers),
                        Some(server_address) => {
                            self.state = ClientState::SendingConnectionRequest;
                            self.server_addr = server_address;
                            self.connect_start_time = self.current_time;
                            self.last_packet_send_time = None;
                            self.last_packet_received_time = self.current_time;
                            self.challenge_token_sequence = 0;

                            return Ok(());
                        }
                    }
                }
                Ok(())
            }
            ClientState::Connected => {
                if connection_timed_out {
                    self.state = ClientState::Disconnected(DisconnectReason::ConnectionTimedOut);
                    return Err(NetcodeError::Disconnected(DisconnectReason::ConnectionTimedOut));
                }

                Ok(())
            }
            ClientState::Disconnected(reason) => Err(NetcodeError::Disconnected(reason)),
        }
    }

    fn generate_packet(&mut self) -> Option<(&mut [u8], SocketAddr)> {
        if let Some(last_packet_send_time) = self.last_packet_send_time {
            if self.current_time - last_packet_send_time < self.send_rate {
                return None;
            }
        }

        if matches!(
            self.state,
            ClientState::Connected | ClientState::SendingConnectionRequest | ClientState::SendingConnectionResponse
        ) {
            self.last_packet_send_time = Some(self.current_time);
        }
        let packet = match self.state {
            ClientState::SendingConnectionRequest => Packet::connection_request_from_token(&self.connect_token),
            ClientState::SendingConnectionResponse => Packet::Response {
                token_sequence: self.challenge_token_sequence,
                token_data: self.challenge_token_data,
            },
            ClientState::Connected => Packet::KeepAlive {
                client_index: 0,
                max_clients: 0,
            },
            _ => return None,
        };

        let result = packet.encode(
            &mut self.out,
            self.connect_token.protocol_id,
            Some((self.sequence, &self.connect_token.client_to_server_key)),
        );
        match result {
            Err(_) => None,
            Ok(encoded) => {
                self.sequence += 1;
                Some((&mut self.out[..encoded], self.server_addr))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{crypto::generate_random_bytes, NETCODE_MAX_PACKET_BYTES};

    use super::*;

    #[test]
    fn client_connection() {
        let mut buffer = [0u8; NETCODE_MAX_PACKET_BYTES];
        let server_addresses: Vec<SocketAddr> = vec!["127.0.0.1:8080".parse().unwrap(), "127.0.0.2:3000".parse().unwrap()];
        let user_data = generate_random_bytes();
        let private_key = b"an example very very secret key."; // 32-bytes
        let protocol_id = 2;
        let expire_seconds = 3;
        let client_id = 4;
        let timeout_seconds = 5;
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
        let server_key = connect_token.server_to_client_key;
        let client_key = connect_token.client_to_server_key;
        let mut client = NetcodeClient::new(Duration::ZERO, connect_token);
        let (packet_buffer, _) = client.update(Duration::ZERO).unwrap();

        let (r_sequence, packet) = Packet::decode(packet_buffer, protocol_id, None, None).unwrap();
        assert_eq!(0, r_sequence);
        assert!(matches!(packet, Packet::ConnectionRequest { .. }));

        let challenge_sequence = 7;
        let user_data = generate_random_bytes();
        let challenge_key = generate_random_bytes();
        let challenge_packet = Packet::generate_challenge(client_id, &user_data, challenge_sequence, &challenge_key).unwrap();
        let len = challenge_packet.encode(&mut buffer, protocol_id, Some((0, &server_key))).unwrap();
        client.process_packet(&mut buffer[..len]);
        assert_eq!(ClientState::SendingConnectionResponse, client.state);

        let (packet_buffer, _) = client.update(Duration::ZERO).unwrap();
        let (_, packet) = Packet::decode(packet_buffer, protocol_id, Some(&client_key), None).unwrap();
        assert!(matches!(packet, Packet::Response { .. }));

        let max_clients = 4;
        let client_index = 2;
        let keep_alive_packet = Packet::KeepAlive { max_clients, client_index };
        let len = keep_alive_packet.encode(&mut buffer, protocol_id, Some((1, &server_key))).unwrap();
        client.process_packet(&mut buffer[..len]);

        assert_eq!(client.state, ClientState::Connected);

        let payload = vec![7u8; 500];
        let payload_packet = Packet::Payload(&payload[..]);
        let len = payload_packet.encode(&mut buffer, protocol_id, Some((2, &server_key))).unwrap();

        let payload_client = client.process_packet(&mut buffer[..len]).unwrap();
        assert_eq!(payload, payload_client);

        let to_send_payload = vec![5u8; 1000];
        let (_, packet) = client.generate_payload_packet(&to_send_payload).unwrap();
        let (_, result) = Packet::decode(packet, protocol_id, Some(&client_key), None).unwrap();
        match result {
            Packet::Payload(payload) => assert_eq!(to_send_payload, payload),
            _ => unreachable!(),
        }
    }
}
