use crate::connection::ClientId;
use crate::error::RenetError;
use crate::protocol::{AuthenticationProtocol, SecurityService, ServerAuthenticationProtocol};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use log::{info, debug};
use std::io;

// TODO: Add version verification
#[derive(Debug, Eq, PartialEq)]
enum Packet {
    ConnectionRequest(ClientId),
    ConnectionDenied,
    KeepAlive,
    Payload(Box<[u8]>),
    Disconnect,
}

#[repr(C)]
enum PacketId {
    ConnectionRequest = 1,
    ConnectionDenied = 2,
    KeepAlive = 3,
    Payload = 4,
    Disconnect = 5,
}

impl PacketId {
    fn from_u8(id: u8) -> Result<Self, ConnectionError> {
        let packet_id = match id {
            1 => Self::ConnectionRequest,
            2 => Self::ConnectionDenied,
            3 => Self::KeepAlive,
            4 => Self::Payload,
            5 => Self::Disconnect,
            _ => return Err(ConnectionError::InvalidPacket),
        };

        Ok(packet_id)
    }
}

// TODO: Refactor with error crate
#[derive(Debug)]
pub enum ConnectionError {
    Denied,
    IOError(io::Error),
    InvalidPacket,
    ClientAlreadyConnected,
    ClientDisconnected,
}

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl std::error::Error for ConnectionError {
    fn description(&self) -> &str {
        "Connection Error"
    }
}

impl From<io::Error> for ConnectionError {
    fn from(inner: io::Error) -> ConnectionError {
        ConnectionError::IOError(inner)
    }
}

impl From<ConnectionError> for RenetError {
    fn from(err: ConnectionError) -> Self {
        RenetError::AuthenticationError(Box::new(err))
    }
}

impl Packet {
    pub fn id(&self) -> PacketId {
        match *self {
            Packet::ConnectionRequest(_) => PacketId::ConnectionRequest,
            Packet::ConnectionDenied => PacketId::ConnectionDenied,
            Packet::KeepAlive => PacketId::KeepAlive,
            Packet::Disconnect => PacketId::Disconnect,
            Packet::Payload(_) => PacketId::Payload,
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Packet::ConnectionRequest(_) => 9,
            Packet::Payload(ref p) => 1 + p.len(),
            _ => 1,
        }
    }

    fn write<W>(&self, out: &mut W) -> Result<(), io::Error>
    where
        W: io::Write,
    {
        match *self {
            Packet::ConnectionRequest(p) => out.write_u64::<BigEndian>(p),
            Packet::Payload(ref p) => out.write_all(p),
            _ => Ok(()),
        }
    }

    pub fn encode(&self, mut buffer: &mut [u8]) -> Result<(), ConnectionError> {
        buffer.write_u8(self.id() as u8)?;
        self.write(&mut buffer)?;
        Ok(())
    }

    pub fn decode(mut buffer: &[u8]) -> Result<Packet, ConnectionError> {
        let packet_type = buffer.read_u8()?;
        let packet_type = PacketId::from_u8(packet_type)?;

        match packet_type {
            PacketId::ConnectionRequest => {
                let client_id = buffer.read_u64::<BigEndian>()?;
                Ok(Packet::ConnectionRequest(client_id))
            }
            PacketId::Payload => {
                let payload = buffer[..buffer.len()].to_vec().into_boxed_slice();
                Ok(Packet::Payload(payload))
            }
            PacketId::Disconnect => Ok(Packet::Disconnect),
            PacketId::KeepAlive => Ok(Packet::KeepAlive),
            PacketId::ConnectionDenied => Ok(Packet::ConnectionDenied),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ClientState {
    Accepted,
    Denied,
    SendingConnectionRequest,
    // TimedOut,
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
    fn ss_wrap(&mut self, data: Box<[u8]>) -> Result<Box<[u8]>, RenetError> {
        let packet = Packet::Payload(data);
        // TODO: can we remove this buffer?
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        Ok(buffer.into_boxed_slice())
    }

    fn ss_unwrap(&mut self, data: Box<[u8]>) -> Result<Box<[u8]>, RenetError> {
        let packet = Packet::decode(&data)?;
        match packet {
            Packet::Payload(payload) => {
                return Ok(payload);
            }
            p => {
                debug!("Received invalid packet: {:?}", p);
                return Err(ConnectionError::InvalidPacket.into());
            }
        }
    }
}

impl AuthenticationProtocol for UnsecureClientProtocol {
    fn id(&self) -> ClientId {
        self.id
    }

    fn create_payload(&mut self) -> Result<Option<Box<[u8]>>, RenetError> {
        let packet = match self.state {
            ClientState::SendingConnectionRequest => Packet::ConnectionRequest(self.id),
            ClientState::Accepted => Packet::KeepAlive,
            _ => return Ok(None),
        };
        // TODO: create buffer inside struct
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        Ok(Some(buffer.into_boxed_slice()))
    }

    fn read_payload(&mut self, payload: Box<[u8]>) -> Result<(), RenetError> {
        let packet = Packet::decode(&payload)?;
        // TODO: better debug logs
        match (packet, &self.state) {
            (Packet::KeepAlive, ClientState::SendingConnectionRequest) => {
                debug!("Received KeepAlive, moving to State Accepted");
                self.state = ClientState::Accepted;
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
        return self.state == ClientState::Accepted;
    }

    fn build_security_interface(&self) -> Box<dyn SecurityService> {
        return Box::new(UnsecureService);
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ServerState {
    SendingKeepAlive,
    Accepted,
    // TimedOut,
}

pub struct UnsecureServerProtocol {
    id: ClientId,
    state: ServerState,
}

impl AuthenticationProtocol for UnsecureServerProtocol {
    fn id(&self) -> ClientId {
        self.id
    }

    fn create_payload(&mut self) -> Result<Option<Box<[u8]>>, RenetError> {
        let packet = match self.state {
            ServerState::SendingKeepAlive => Packet::KeepAlive,
            _ => return Ok(None),
        };

        // TODO: create buffer inside struct
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        Ok(Some(buffer.into_boxed_slice()))
    }

    fn read_payload(&mut self, payload: Box<[u8]>) -> Result<(), RenetError> {
        let packet = Packet::decode(&payload)?;
        // TODO: better debug logs
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
        return self.state == ServerState::Accepted;
    }

    fn build_security_interface(&self) -> Box<dyn SecurityService> {
        return Box::new(UnsecureService);
    }
}

impl ServerAuthenticationProtocol for UnsecureServerProtocol {
    fn from_payload(payload: Box<[u8]>) -> Result<Box<dyn AuthenticationProtocol>, RenetError> {
        let packet = Packet::decode(&payload)?;
        if let Packet::ConnectionRequest(client_id) = packet {
            return Ok(Box::new(Self {
                id: client_id,
                state: ServerState::SendingKeepAlive,
            }));
        }
        Err(ConnectionError::InvalidPacket.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_success() {
        let mut client_protocol = UnsecureClientProtocol::new(1);

        let connection_payload = client_protocol.create_payload().unwrap().unwrap();

        let mut server_protocol = UnsecureServerProtocol::from_payload(connection_payload).unwrap();

        let server_keep_alive_payload = server_protocol.create_payload().unwrap().unwrap();

        client_protocol.read_payload(server_keep_alive_payload).unwrap();
        
        assert!(client_protocol.is_authenticated(), "Client protocol should be authenticated!");
        let client_keep_alive_payload = client_protocol.create_payload().unwrap().unwrap();

        server_protocol.read_payload(client_keep_alive_payload).unwrap();
        assert!(server_protocol.is_authenticated(), "Server protocol should be authenticated!");

        let mut client_ss =  client_protocol.build_security_interface();
        let mut server_ss = server_protocol.build_security_interface();

        let payload = vec![0, 1, 2, 3, 4, 5, 6, 7, 8 ,9];
        
        let wrapped_payload = server_ss.ss_wrap(payload.clone().into_boxed_slice()).unwrap();

        let unwrapped_payload = client_ss.ss_unwrap(wrapped_payload).unwrap();
        
        assert_eq!(unwrapped_payload, payload.into_boxed_slice());
    }
}
