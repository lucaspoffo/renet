use crate::error::RenetError;
use crate::protocol::{AuthenticationProtocol, SecurityService, ServerAuthenticationProtocol};
use crate::remote_connection::ClientId;

use log::debug;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// TODO: Add version verification
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Packet {
    ConnectionRequest(ClientId),
    ConnectionDenied,
    KeepAlive,
    Payload(Vec<u8>),
    Disconnect,
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("received invalid protocol packet")]
    InvalidPacket,
}

#[derive(Debug, Eq, PartialEq)]
enum ClientState {
    Accepted,
    Confirmed,
    Denied,
    SendingConnectionRequest,
}

pub struct UnsecureClientProtocol {
    id: ClientId,
    state: ClientState,
}

pub struct UnsecureService;

impl UnsecureClientProtocol {
    pub fn new(id: ClientId) -> Self {
        Self {
            id,
            state: ClientState::SendingConnectionRequest,
        }
    }
}

impl SecurityService for UnsecureService {
    fn ss_wrap(&mut self, data: &[u8]) -> Result<Vec<u8>, RenetError> {
        let packet = Packet::Payload(data.into());
        Ok(bincode::serialize(&packet)?)
    }

    fn ss_unwrap(&mut self, data: &[u8]) -> Result<Vec<u8>, RenetError> {
        let packet = bincode::deserialize(data)?;
        match packet {
            Packet::Payload(payload) => Ok(payload),
            _ => Err(RenetError::AuthenticationError(Box::new(
                ConnectionError::InvalidPacket,
            ))),
        }
    }
}

impl AuthenticationProtocol for UnsecureClientProtocol {
    type Service = UnsecureService;

    fn id(&self) -> ClientId {
        self.id
    }

    fn create_payload(&mut self) -> Result<Option<Vec<u8>>, RenetError> {
        let packet = match self.state {
            ClientState::SendingConnectionRequest => Packet::ConnectionRequest(self.id),
            ClientState::Accepted => {
                // Send one confirmation KeepAlive packet always
                self.state = ClientState::Confirmed;
                Packet::KeepAlive
            }
            _ => return Ok(None),
        };

        let packet = bincode::serialize(&packet)?;
        Ok(Some(packet))
    }

    fn read_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        let packet = bincode::deserialize(payload)?;
        match (packet, &self.state) {
            (Packet::KeepAlive, ClientState::SendingConnectionRequest) => {
                debug!("Received KeepAlive, moving to State Accepted");
                self.state = ClientState::Accepted;
            }
            (Packet::KeepAlive, ClientState::Accepted) => {
                debug!("Received KeepAlive, but state is already Accepted");
            }
            (Packet::ConnectionDenied, _) => {
                debug!("Received ConnectionDenied Package");
                self.state = ClientState::Denied;
            }
            (p, _) => debug!("Received invalid packet {:?}", p),
        }
        Ok(())
    }

    fn is_authenticated(&self) -> bool {
        self.state == ClientState::Confirmed
    }

    fn build_security_interface(&self) -> Self::Service {
        UnsecureService
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ServerState {
    SendingKeepAlive,
    Accepted,
}

pub struct UnsecureServerProtocol {
    id: ClientId,
    state: ServerState,
}

impl AuthenticationProtocol for UnsecureServerProtocol {
    type Service = UnsecureService;

    fn id(&self) -> ClientId {
        self.id
    }

    fn create_payload(&mut self) -> Result<Option<Vec<u8>>, RenetError> {
        let packet = match self.state {
            ServerState::SendingKeepAlive => Packet::KeepAlive,
            _ => return Ok(None),
        };

        Ok(Some(bincode::serialize(&packet)?))
    }

    fn read_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        let packet = bincode::deserialize(payload)?;
        match (packet, &self.state) {
            (Packet::ConnectionRequest(_), _) => {}
            (Packet::KeepAlive, ServerState::SendingKeepAlive) => {
                debug!("Received KeepAlive from {}, accepted connection.", self.id);
                self.state = ServerState::Accepted;
            }
            (Packet::Payload(_), ServerState::SendingKeepAlive) => {
                debug!("Received Payload from {}, accepted connection.", self.id);
                self.state = ServerState::Accepted;
            }
            _ => {}
        }

        Ok(())
    }

    fn is_authenticated(&self) -> bool {
        self.state == ServerState::Accepted
    }

    fn build_security_interface(&self) -> Self::Service {
        UnsecureService
    }
}

impl ServerAuthenticationProtocol for UnsecureServerProtocol {
    fn from_payload(payload: &[u8]) -> Result<Self, RenetError> {
        let packet = bincode::deserialize(payload)?;
        if let Packet::ConnectionRequest(client_id) = packet {
            Ok(Self {
                id: client_id,
                state: ServerState::SendingKeepAlive,
            })
        } else {
            Err(RenetError::AuthenticationError(Box::new(
                ConnectionError::InvalidPacket,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_success() {
        let mut client_protocol = UnsecureClientProtocol::new(1);

        let connection_payload = client_protocol.create_payload().unwrap().unwrap();

        let mut server_protocol =
            UnsecureServerProtocol::from_payload(&connection_payload).unwrap();

        let server_keep_alive_payload = server_protocol.create_payload().unwrap().unwrap();

        client_protocol
            .read_payload(&server_keep_alive_payload)
            .unwrap();

        let client_keep_alive_payload = client_protocol.create_payload().unwrap().unwrap();

        assert!(
            client_protocol.is_authenticated(),
            "Client protocol should be authenticated!"
        );

        server_protocol
            .read_payload(&client_keep_alive_payload)
            .unwrap();
        assert!(
            server_protocol.is_authenticated(),
            "Server protocol should be authenticated!"
        );

        let mut client_ss = client_protocol.build_security_interface();
        let mut server_ss = server_protocol.build_security_interface();

        let payload = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

        let wrapped_payload = server_ss.ss_wrap(&payload).unwrap();

        let unwrapped_payload = client_ss.ss_unwrap(&wrapped_payload).unwrap();

        assert_eq!(unwrapped_payload, payload);
    }
}
