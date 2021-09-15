use crate::error::DisconnectionReason;
use crate::packet::Payload;
use crate::protocol::{AuthenticationProtocol, Result, ServerAuthenticationProtocol};
use crate::ClientId;

use bincode::Options;
use log::debug;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// TODO: Add version verification
#[derive(Debug, Clone, Serialize, Deserialize)]
enum ProtocolMessage<C> {
    ConnectionRequest(C),
    ConnectionFailed(DisconnectionReason),
    KeepAlive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum ProtocolPacket<C> {
    Unauthenticaded(ProtocolMessage<C>),
    Authenticated(Payload),
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("received invalid protocol packet")]
    InvalidPacket,
}

#[derive(Debug)]
enum ClientState {
    Accepted,
    Failed { reason: DisconnectionReason },
    SendingConfirmed,
    SendingConnectionRequest,
}

pub struct UnsecureClientProtocol<C> {
    id: C,
    state: ClientState,
}

impl<C> UnsecureClientProtocol<C> {
    pub fn new(id: C) -> Self {
        Self {
            id,
            state: ClientState::SendingConnectionRequest,
        }
    }
}

impl<C: ClientId + Serialize + DeserializeOwned> AuthenticationProtocol
    for UnsecureClientProtocol<C>
{
    type ClientId = C;

    fn id(&self) -> C {
        self.id
    }

    fn create_payload(&mut self) -> Result<Option<Payload>> {
        let packet = match self.state {
            ClientState::SendingConnectionRequest => {
                ProtocolPacket::Unauthenticaded(ProtocolMessage::ConnectionRequest(self.id))
            }
            ClientState::SendingConfirmed => {
                self.state = ClientState::Accepted;
                ProtocolPacket::Unauthenticaded(ProtocolMessage::KeepAlive)
            }
            _ => return Ok(None),
        };

        let packet = bincode::options().serialize(&packet)?;
        Ok(Some(packet))
    }

    fn wrap(&mut self, payload: &[u8]) -> Result<Payload> {
        let packet = ProtocolPacket::<C>::Authenticated(payload.to_vec());
        Ok(bincode::options().serialize(&packet)?)
    }

    fn process(&mut self, payload: &[u8]) -> Option<Payload> {
        let packet: ProtocolPacket<C> = bincode::options().deserialize(payload).ok()?;

        match packet {
            ProtocolPacket::Unauthenticaded(message) => match (message, &self.state) {
                (ProtocolMessage::KeepAlive, ClientState::SendingConnectionRequest) => {
                    debug!("Received KeepAlive.");
                    self.state = ClientState::SendingConfirmed;
                }
                (
                    ProtocolMessage::ConnectionFailed(reason),
                    ClientState::SendingConnectionRequest,
                ) => {
                    debug!("Received ConnectionFailed {}", reason);
                    self.state = ClientState::Failed { reason };
                }
                (ProtocolMessage::ConnectionFailed(reason), ClientState::SendingConfirmed) => {
                    debug!("Received ConnectionFailed {}", reason);
                    self.state = ClientState::Failed { reason };
                }
                (ProtocolMessage::KeepAlive, ClientState::Accepted) => {
                    debug!("Received KeepAlive, but state is already Accepted");
                }
                (p, _) => debug!("Received invalid packet {:?}", p),
            },
            ProtocolPacket::Authenticated(payload) => match self.state {
                ClientState::Accepted => {
                    return Some(payload);
                }
                _ => debug!("Received Authenticated packet, but state is not accepted"),
            },
        }
        None
    }

    fn is_authenticated(&self) -> bool {
        matches!(self.state, ClientState::Accepted)
    }

    fn disconnected(&self) -> Option<DisconnectionReason> {
        match self.state {
            ClientState::Failed { reason } => Some(reason),
            _ => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ServerState {
    SendingKeepAlive,
    Accepted,
}

pub struct UnsecureServerProtocol<C> {
    id: C,
    state: ServerState,
}

impl<C: ClientId + Serialize + DeserializeOwned> AuthenticationProtocol
    for UnsecureServerProtocol<C>
{
    type ClientId = C;

    fn id(&self) -> C {
        self.id
    }

    fn create_payload(&mut self) -> Result<Option<Vec<u8>>> {
        let packet = match self.state {
            ServerState::SendingKeepAlive => {
                ProtocolPacket::Unauthenticaded(ProtocolMessage::<C>::KeepAlive)
            }
            _ => return Ok(None),
        };

        Ok(Some(bincode::options().serialize(&packet)?))
    }

    fn wrap(&mut self, payload: &[u8]) -> Result<Payload> {
        let packet = ProtocolPacket::<C>::Authenticated(payload.to_vec());
        Ok(bincode::options().serialize(&packet)?)
    }

    fn process(&mut self, payload: &[u8]) -> Option<Payload> {
        let packet: ProtocolPacket<C> = bincode::options().deserialize(payload).ok()?;
        match packet {
            ProtocolPacket::Unauthenticaded(message) => match (message, &self.state) {
                (ProtocolMessage::KeepAlive, ServerState::SendingKeepAlive) => {
                    debug!(
                        "Received KeepAlive from {:?}, accepted connection.",
                        self.id
                    );
                    self.state = ServerState::Accepted;
                }
                _ => {
                    debug!("Discarting Unauthenticaded packet")
                }
            },
            ProtocolPacket::Authenticated(payload) => match self.state {
                ServerState::Accepted => {
                    return Some(payload);
                }
                ServerState::SendingKeepAlive => {
                    debug!("Received Authenticated Packet but was waiting KeepAlive");
                    self.state = ServerState::Accepted;
                    return Some(payload);
                }
            },
        }

        None
    }

    fn is_authenticated(&self) -> bool {
        self.state == ServerState::Accepted
    }

    fn disconnected(&self) -> Option<DisconnectionReason> {
        // TODO:
        None
    }
}

impl<C: ClientId + Serialize + DeserializeOwned> ServerAuthenticationProtocol
    for UnsecureServerProtocol<C>
{
    fn get_disconnect_packet(reason: DisconnectionReason) -> Payload {
        let packet =
            ProtocolPacket::<C>::Unauthenticaded(ProtocolMessage::ConnectionFailed(reason));
        // Should we return an error when failing to serialize?
        bincode::options().serialize(&packet).unwrap()
    }

    fn from_payload(payload: &[u8]) -> Result<Self> {
        let packet: ProtocolPacket<C> = bincode::options().deserialize(payload)?;
        if let ProtocolPacket::Unauthenticaded(ProtocolMessage::ConnectionRequest(client_id)) =
            packet
        {
            Ok(Self {
                id: client_id,
                state: ServerState::SendingKeepAlive,
            })
        } else {
            Err(Box::new(ConnectionError::InvalidPacket))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_success() {
        let mut client_protocol: UnsecureClientProtocol<u64> = UnsecureClientProtocol::new(1);

        let connection_payload = client_protocol.create_payload().unwrap().unwrap();

        let mut server_protocol =
            UnsecureServerProtocol::<u64>::from_payload(&connection_payload).unwrap();

        let server_keep_alive_payload = server_protocol.create_payload().unwrap().unwrap();

        client_protocol.process(&server_keep_alive_payload);

        let client_keep_alive_payload = client_protocol.create_payload().unwrap().unwrap();

        assert!(
            client_protocol.is_authenticated(),
            "Client protocol should be authenticated!"
        );

        server_protocol.process(&client_keep_alive_payload);
        assert!(
            server_protocol.is_authenticated(),
            "Server protocol should be authenticated!"
        );

        let payload = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

        let wrapped_payload = server_protocol.wrap(&payload).unwrap();

        let unwrapped_payload = client_protocol.process(&wrapped_payload).unwrap();

        assert_eq!(unwrapped_payload, payload);
    }
}
