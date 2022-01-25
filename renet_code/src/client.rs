use std::{net::SocketAddr, time::Duration};

use crate::{
    packet::{ConnectionKeepAlive, ConnectionRequest, ConnectionResponse, NetcodeError, Packet},
    token::{ConnectToken, PrivateConnectToken},
    NETCODE_CHALLENGE_TOKEN_BYTES, NETCODE_SEND_RATE, NETCODE_VERSION_INFO,
};

#[derive(Debug, PartialEq, Eq)]
pub enum ErrorReason {
    ConnectTokenExpired,
    InvalidConnectToken,
    ConnectionTimedOut,
    ConnectionResponseTimedOut,
    ConnectionRequestTimedOut,
    ConnectionDenied,
}

#[derive(Debug, PartialEq, Eq)]
pub enum State {
    Error(ErrorReason),
    Disconnected,
    SendingConnectionRequest,
    SendingConnectionResponse,
    Connected,
}

#[derive(Debug)]
pub struct Client {
    state: State,
    connect_start_time: Duration,
    last_packet_send_time: Option<Duration>,
    last_packet_received_time: Duration,
    current_time: Duration,
    sequence: u64,
    server_address: SocketAddr,
    server_address_index: usize,
    connect_token: ConnectToken,
    challenge_token_sequence: u64,
    challenge_token_data: [u8; NETCODE_CHALLENGE_TOKEN_BYTES],
    max_clients: u32,
    client_index: u32,
    send_rate: Duration,
}

impl Client {
    pub fn new(current_time: Duration, connect_token: ConnectToken) -> Self {
        let server_address = connect_token.server_addresses[0].unwrap();

        Self {
            sequence: 0,
            server_address,
            server_address_index: 0,
            challenge_token_sequence: 0,
            state: State::SendingConnectionRequest,
            current_time,
            connect_start_time: Duration::ZERO,
            last_packet_send_time: None,
            last_packet_received_time: Duration::ZERO,
            max_clients: 0,
            client_index: 0,
            send_rate: NETCODE_SEND_RATE,
            challenge_token_data: [0u8; NETCODE_CHALLENGE_TOKEN_BYTES],
            connect_token,
        }
    }

    pub fn process_packet<'a>(&mut self, packet: Packet<'a>) -> Option<&'a [u8]> {
        match (packet, &self.state) {
            (Packet::ConnectionDenied, State::SendingConnectionRequest | State::SendingConnectionResponse) => {
                self.state = State::Error(ErrorReason::ConnectionDenied);
                self.last_packet_received_time = self.current_time;
            }
            (Packet::Challenge(challenge), State::SendingConnectionRequest) => {
                self.challenge_token_sequence = challenge.token_sequence;
                self.last_packet_received_time = self.current_time;
                self.last_packet_send_time = None;
                self.challenge_token_data = challenge.token_data;
                self.state = State::SendingConnectionResponse;
            }
            (Packet::KeepAlive(_), State::Connected) => {
                self.last_packet_received_time = self.current_time;
            }
            (Packet::KeepAlive(keep_alive), State::SendingConnectionResponse) => {
                self.last_packet_received_time = self.current_time;
                self.max_clients = keep_alive.max_clients;
                self.client_index = keep_alive.client_index;
                self.state = State::Connected;
            }
            (Packet::Payload(p), State::Connected) => {
                self.last_packet_received_time = self.current_time;
                return Some(p);
            }
            (Packet::Disconnect, State::Connected) => {
                self.state = State::Disconnected;
                self.last_packet_received_time = self.current_time;
            }
            _ => {}
        }

        None
    }

    fn connect_to_next_server(&mut self) -> Result<(), NetcodeError> {
        self.server_address_index += 1;
        if self.server_address_index >= 32 {
            return Err(NetcodeError::NoMoreServers);
        }
        match self.connect_token.server_addresses[self.server_address_index] {
            None => Err(NetcodeError::NoMoreServers),
            Some(server_address) => {
                self.state = State::SendingConnectionRequest;
                self.server_address = server_address;
                self.connect_start_time = self.current_time;
                self.last_packet_send_time = None;
                self.last_packet_received_time = self.current_time;
                self.challenge_token_sequence = 0;

                Ok(())
            }
        }
    }

    fn generate_packet(&mut self) -> Option<Packet> {
        if let Some(last_packet_send_time) = self.last_packet_send_time {
            if self.current_time - last_packet_send_time < self.send_rate {
                return None;
            }
        }

        if matches!(self.state, State::SendingConnectionRequest | State::SendingConnectionResponse) {
            self.last_packet_send_time = Some(self.current_time);
        }

        match self.state {
            State::SendingConnectionRequest => Some(Packet::ConnectionRequest(ConnectionRequest::from_token(&self.connect_token))),
            State::SendingConnectionResponse => Some(Packet::Response(ConnectionResponse {
                token_sequence: self.challenge_token_sequence,
                token_data: self.challenge_token_data,
            })),
            State::Connected => Some(Packet::KeepAlive(ConnectionKeepAlive {
                client_index: 0,
                max_clients: 0,
            })),
            _ => None,
        }
    }

    pub fn update(&mut self) -> Result<(), NetcodeError> {
        let connection_timed_out = self.connect_token.timeout_seconds > 0
            && (self.last_packet_received_time + Duration::from_secs(self.connect_token.timeout_seconds as u64) < self.current_time);

        match self.state {
            State::SendingConnectionRequest | State::SendingConnectionResponse => {
                let expire_seconds = self.connect_token.expire_timestamp - self.connect_token.create_timestamp;
                let connection_expired = (self.current_time - self.connect_start_time).as_secs() >= expire_seconds;
                if connection_expired {
                    self.state = State::Error(ErrorReason::ConnectTokenExpired);
                    return Err(NetcodeError::Expired);
                }
                if connection_timed_out {
                    let reason = if self.state == State::SendingConnectionResponse {
                        ErrorReason::ConnectionResponseTimedOut
                    } else {
                        ErrorReason::ConnectionRequestTimedOut
                    };
                    self.state = State::Error(reason);
                    return self.connect_to_next_server();
                }

                Ok(())
            }
            State::Connected => {
                if connection_timed_out {
                    self.state = State::Error(ErrorReason::ConnectionTimedOut);
                    return Err(NetcodeError::TimedOut);
                }

                Ok(())
            }
            State::Disconnected | State::Error(_) => Err(NetcodeError::Disconnected),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{crypto::generate_random_bytes, packet::ConnectionChallenge};

    use super::*;

    #[test]
    fn client_connection() {
        let server_addresses: Vec<SocketAddr> = vec!["127.0.0.1:8080".parse().unwrap(), "127.0.0.2:3000".parse().unwrap()];
        let user_data = generate_random_bytes();
        let private_key = b"an example very very secret key."; // 32-bytes
        let sequence = 1;
        let protocol_id = 2;
        let expire_seconds = 3;
        let client_id = 4;
        let timeout_seconds = 5;
        let connect_token = ConnectToken::generate(
            sequence,
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
        let packet = client.generate_packet().unwrap();

        assert!(matches!(packet, Packet::ConnectionRequest(_)));

        let challenge_sequence = 7;
        let user_data = generate_random_bytes();
        let challenge_key = generate_random_bytes();
        let challenge_packet =
            Packet::Challenge(ConnectionChallenge::generate(client_id, &user_data, challenge_sequence, &challenge_key).unwrap());
        client.process_packet(challenge_packet);

        assert_eq!(client.state, State::SendingConnectionResponse);
        let packet = client.generate_packet().unwrap();
        assert!(matches!(packet, Packet::Response(_)));

        let max_clients = 4;
        let client_index = 2;
        let keep_alive_packet = Packet::KeepAlive(ConnectionKeepAlive { max_clients, client_index });
        client.process_packet(keep_alive_packet);

        assert_eq!(client.state, State::Connected);

        let payload = vec![7u8; 500];
        let payload_packet = Packet::Payload(&payload[..]);

        let payload_client = client.process_packet(payload_packet).unwrap();
        assert_eq!(payload, payload_client);
    }
}
