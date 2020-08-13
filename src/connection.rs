use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Instant, Duration};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
// TODO: Setup Client / Server arquitecture
// Add ClientState
// Connecting/Connected/Disconnected/Denied/TimedOut/RequestTimeOut/ResponseTimeOut
// Client Internal State (Connecting/Connected/Disconnected)
//
// New PacketTypes
//  ConnectiongRequest / ConnectionDenied / Challenge / HeartBeat / Payload / Disconnect
//
// Client Events
//
// Server Events:
//  ClientConnected, ClientDisconnected, ClientSlotFull, Packet, SentKeepAlive, RejectClient
//
// ServerInternal

// Client Side
//
// Client sends connection request until receiving challenge packet,
// When challenged sends challenge responce until receive a connection keep-alive packet
// Check timeout to move to error
//
// Server Side
//
// Client must have valid connect token
// Ignore malformed requests
//
// Resolving connection packet
// check expected size
// check version info
// check protocol id
// check token expiration time
// check encription
// ignore if client already connected
// check if the server is full
// respond with a Connection Challenge Packet
//
// Processing Connection Response Packet
// if the encrypted challenge token data fails to decrypt, ignore packet,
// if client already connected, ignore packet
// if client slots full, respond with Connection Denied Packet,
// Assign packet SocketAddr and ClientId to a server free slot,
// Mark client as connected,
// Respond with Keep-Alive packet,
//
// Connected Clients
// Accepts packets:
//  keep-alive
//  payload
//  disconnect
//
// In the absence of payload packets the server send Keep-Alive packets at some rate like 10Hz
//

const PACKET_CONNECTION: u8 = 0;
const PACKET_CONNECTION_DENIED: u8 = 1;
const PACKET_CHALLENGE: u8 = 2;
const PACKET_CHALLENGE_RESPONSE: u8 = 3;
const PACKET_HEARTBEAT: u8 = 4;
const PACKET_PAYLOAD: u8 = 5;
const PACKET_DISCONNECT: u8 = 6;

#[derive(Debug)]
pub enum Packet {
    ConnectionRequest(ClientId),
    ConnectionDenied,
    Challenge,
    ChallengeResponse,
    HeartBeat,
    Disconnect,
}

impl Packet {
    pub fn id(&self) -> u8 {
        match *self {
            Packet::ConnectionRequest(_) => PACKET_CONNECTION,
            Packet::ConnectionDenied => PACKET_CONNECTION_DENIED,
            Packet::Challenge => PACKET_CHALLENGE,
            Packet::ChallengeResponse => PACKET_CHALLENGE_RESPONSE,
            Packet::HeartBeat => PACKET_HEARTBEAT,
            Packet::Disconnect => PACKET_DISCONNECT,
        }
    }

    fn write<W>(&self, out: &mut W) -> Result<(), io::Error>
    where
        W: io::Write,
    {
        match *self {
            Packet::ConnectionRequest(p) => out.write_u64::<BigEndian>(p), 
            // Packet::Challenge(ref p) => p.write(out),
            // Packet::Response(ref p) => p.write(out),
            // Packet::KeepAlive(ref p) => p.write(out),
            // Packet::ConnectionDenied | Packet::Payload(_) | Packet::Disconnect => Ok(()),
            _ => Ok(()),
        }
    }

    fn encode(&self, mut buffer: &mut [u8]) -> Result<(), ConnectionError> {
        buffer.write_u8(self.id())?;
        self.write(&mut buffer)?;

        Ok(())
    }

    fn decode(mut buffer: &[u8]) -> Result<Packet, ConnectionError> {
        let packet_type = buffer.read_u8()?;
        if packet_type == PACKET_CONNECTION {
            let client_id = buffer.read_u64::<BigEndian>()?;
            return Ok(Packet::ConnectionRequest(client_id));
        }

        match packet_type {
            PACKET_DISCONNECT => Ok(Packet::Disconnect),
            PACKET_HEARTBEAT => Ok(Packet::HeartBeat),
            PACKET_CHALLENGE => Ok(Packet::Challenge),
            PACKET_CONNECTION_DENIED => Ok(Packet::ConnectionDenied),
            PACKET_CHALLENGE_RESPONSE => Ok(Packet::ChallengeResponse),
            _ => Err(ConnectionError::InvalidPacket),
        }
    }
}

pub enum ClientState {
    ConnectionTimedOut,
    ConnectionResponseTimedOut,
    ConnectionRequestTimedOut,
    ConnectionDenied,
    Disconnected,
    SendingConnectionRequest,
    SendingChallengeResponse,
    Connected,
}

struct RequestConnection {
    state: ClientState,
    socket: UdpSocket,
    buffer: [u8; 1500],
    server_addr: SocketAddr,
}

enum ConnectionError {
    Denied,
    IOError(io::Error),
    InvalidPacket,
}

impl From<io::Error> for ConnectionError {
    fn from(inner: io::Error) -> ConnectionError {
        ConnectionError::IOError(inner)
    }
}

impl RequestConnection {
    fn handle_packet(&mut self, packet: Packet) -> Result<(), ConnectionError> {
        match packet {
            Packet::Challenge => {
                self.state = ClientState::SendingChallengeResponse;
                Ok(())
            }
            Packet::HeartBeat => {
                self.state = ClientState::Connected;
                Ok(())
            }
            Packet::ConnectionDenied => {
                self.state = ClientState::ConnectionDenied;
                return Err(ConnectionError::Denied);
            },
            _ => { Ok(()) }
        }
    }

    /*
    fn update(&self) -> Result<ConnectionRequest, RequestConnectionError> {
        match self.state {
            ClientState::
        }
    }
    */

    fn next_event(&mut self) -> Result<(), ConnectionError> {
        let packet = match self.socket.recv_from(&mut self.buffer) {
            Ok((len, addr)) => {
                if addr == self.server_addr {
                    let payload = &self.buffer[..len];
                    let packet = Packet::decode(payload)?;
                    Some(packet)
                } else {
                    log::debug!("Discarded packet from unknown host {:?}", addr);
                    None
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => return Err(ConnectionError::IOError(e).into()),
        };

        if let Some(packet) = packet {
            match packet {
                Packet::Challenge => {
                    match self.state {
                        ClientState::SendingConnectionRequest => {
                            log::debug!("Received Challenge Packet, moving to State Sending Response");
                            self.state = ClientState::SendingChallengeResponse;
                        },
                        _ => {},
                    }
                },
                Packet::HeartBeat => {
                    match self.state {
                        ClientState::SendingChallengeResponse => {
                            log::debug!("Received HeartBeat while sending challenge response, successfuly connected");
                            self.state = ClientState::Connected;
                        }
                        _ => {},
                    }
                },
                Packet::ConnectionDenied => {
                    self.state = ClientState::ConnectionDenied;
                },
                p => { log::debug!("Received invalid packet {:?}", p); },
            }
        }

        Ok(())
    }
}

enum ResponseConnectionState {
    SendingChallenge,
    SendingAccepted,
}

struct ResponceConnection {
    state: ResponseConnectionState,
}

type ClientId = u64;

struct Client {
    id: ClientId,
    state: ClientState,
    addr: SocketAddr,
}

struct ServerConfig {
    max_clients: usize,
}

struct Server {
    config: ServerConfig,
    clients: HashMap<ClientId, Client>,
}

enum ServerEvent {
    ClientConnected(ClientId),
    ClientDisconnected(ClientId),
    ServerSlotsFull,
    RejectClient(ClientId),
}

impl Server {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            clients: HashMap::new(),
            config,
        }
    }

    fn find_client_by_addr(&self, addr: &SocketAddr) -> Option<&Client> {
        self.clients.values().find(|c| c.addr == *addr)
    }

    /*
    pub fn handle_packet(&mut self, packet: ConnectionPacket, addr: &SocketAddr) -> std::result::Result<Option<ServerEvent>, ()> {
        if let Some(client) = self.find_client_by_addr(addr) {
        }

        match packet {
            ConnectionPacket::Request(client_id) => {
                if self.clients.get(&client_id).is_some() {
                    debug!("Client {} already exists", client_id);
                    return Ok(None);
                }

                let client = Client::new(client_id, addr.clone());
                self.clients.insert(client_id, client);

                Ok(Some(ServerEvent::ClientConnected(client_id)))
            },
            _ =>
                Ok(None),
        }
    }
    */
}
