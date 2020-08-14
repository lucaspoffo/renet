use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};
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
enum Packet {
    ConnectionRequest(ClientId),
    ConnectionDenied,
    Challenge,
    ChallengeResponse,
    HeartBeat,
    Disconnect,
}

impl Packet {
    fn id(&self) -> u8 {
        match *self {
            Packet::ConnectionRequest(_) => PACKET_CONNECTION,
            Packet::ConnectionDenied => PACKET_CONNECTION_DENIED,
            Packet::Challenge => PACKET_CHALLENGE,
            Packet::ChallengeResponse => PACKET_CHALLENGE_RESPONSE,
            Packet::HeartBeat => PACKET_HEARTBEAT,
            Packet::Disconnect => PACKET_DISCONNECT,
        }
    }
    
    fn size(&self) -> usize {
        match self {
            Packet::ConnectionRequest(_) => 9,
            _ => 1,
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

// TODO: separate connected state from connecting state
#[derive(Debug, Eq, PartialEq)]
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

pub struct RequestConnection {
    state: ClientState,
    socket: UdpSocket,
    buffer: Vec<u8>, 
    server_addr: SocketAddr,
}

#[derive(Debug)]
pub enum ConnectionError {
    Denied,
    IOError(io::Error),
    InvalidPacket,
    ClientAlreadyConnected,
}

impl From<io::Error> for ConnectionError {
    fn from(inner: io::Error) -> ConnectionError {
        ConnectionError::IOError(inner)
    }
}

pub struct ServerConnection;

impl RequestConnection {
    pub fn new(socket: UdpSocket, server_addr: SocketAddr) -> Result<Self, ConnectionError> {
        socket.set_nonblocking(true);
        Ok(Self {
            socket,
            server_addr,
            buffer: vec![0u8; 1500], 
            state: ClientState::SendingConnectionRequest,
        })
    }

    fn send_packet(&self, packet: Packet) -> Result<(), ConnectionError> {
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        log::debug!("Send Packet buffer: {:?}", buffer);
        self.socket.send_to(&buffer, self.server_addr)?;
        Ok(())
    }

    fn handle_packet(&mut self, packet: Packet) -> Result<(), ConnectionError> {
        match packet {
            Packet::Challenge => {
                if self.state == ClientState::SendingConnectionRequest {
                    log::debug!("Received Challenge Packet, moving to State Sending Response");
                    self.state = ClientState::SendingChallengeResponse;
                }
            }
            Packet::HeartBeat => {
                if self.state == ClientState::SendingChallengeResponse {
                    log::debug!("Received HeartBeat while sending challenge response, successfuly connected");
                    self.state = ClientState::Connected;
                }
            }
            Packet::ConnectionDenied => {
                self.state = ClientState::ConnectionDenied;
                return Err(ConnectionError::Denied);
            }
            p => {
                log::debug!("Received invalid packet {:?}", p);
            }
        }
        Ok(())
    }

    pub fn update(&mut self) -> Result<Option<ServerConnection>, ConnectionError> {
        self.process_events()?;
        log::debug!("State: {:?}", self.state);
        match self.state {
            ClientState::SendingConnectionRequest => {
                self.send_packet(Packet::ConnectionRequest(0))?;
            },
            ClientState::SendingChallengeResponse => {
                self.send_packet(Packet::ChallengeResponse)?;
            },
            ClientState::Connected => {
                self.send_packet(Packet::HeartBeat)?;
                return Ok(Some(ServerConnection));
            },
            _ => {}
        }
        Ok(None)
    }

    fn process_events(&mut self) -> Result<(), ConnectionError> {
        loop {
            let packet = match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        let payload = &self.buffer[..len];
                        let packet = Packet::decode(payload)?;
                        packet
                    } else {
                        log::debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };

            self.handle_packet(packet)?;
        }
    }
}

enum ResponseConnectionState {
    SendingChallenge,
    SendingHeartBeat,
    Connected,
}

struct HandleConnection {
    addr: SocketAddr,
    client_id: ClientId,
    state: ResponseConnectionState,
}

impl HandleConnection {
    fn new(client_id: ClientId, addr: SocketAddr) -> Self {
        Self {
            client_id,
            addr,
            state: ResponseConnectionState::SendingChallenge,
        }
    }

    fn process_packet(&mut self, packet: &Packet) {
        match packet {
            Packet::ConnectionRequest(_) => {}
            Packet::ChallengeResponse => {
                if let ResponseConnectionState::SendingChallenge = self.state {
                    //TODO: check if challenge is valid
                    log::debug!("Received Challenge Response from {}.", self.addr);
                    self.state = ResponseConnectionState::SendingHeartBeat;
                }
            }
            Packet::HeartBeat => {
                if let ResponseConnectionState::SendingHeartBeat = self.state {
                    log::debug!(
                        "Received HeartBeat from {}, accepted connection.",
                        self.addr
                    );
                    self.state = ResponseConnectionState::Connected;
                }
            }
            _ => {}
        }
    }
}

type ClientId = u64;

struct Client {
    id: ClientId,
    state: ClientState,
    addr: SocketAddr,
}

impl Client {
    fn new(id: ClientId, addr: SocketAddr) -> Self {
        Self {
            id,
            addr,
            state: ClientState::Connected,
        }
    }
}

pub struct ServerConfig {
    max_clients: usize,
}

impl ServerConfig {
    pub fn new(max_clients: usize) -> Self {
        Self { max_clients }
    }
}

pub struct Server {
    config: ServerConfig,
    socket: UdpSocket,
    clients: HashMap<ClientId, Client>,
    connecting: HashMap<ClientId, HandleConnection>,
    buffer: Vec<u8>,
}

enum ServerEvent {
    ClientConnected(ClientId),
    ClientDisconnected(ClientId),
    ServerSlotsFull,
    RejectClient(ClientId),
}

impl Server {
    pub fn new(socket: UdpSocket, config: ServerConfig) -> Result<Self, ConnectionError> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            clients: HashMap::new(),
            connecting: HashMap::new(),
            config,
            buffer: vec![0u8; 1500],
        })
    }

    fn find_client_by_addr(&self, addr: &SocketAddr) -> Option<&Client> {
        self.clients.values().find(|c| c.addr == *addr)
    }

    fn find_connection_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut HandleConnection> {
        self.connecting.values_mut().find(|c| c.addr == *addr)
    }

    fn send_packet(&self, packet: Packet, addr: &SocketAddr) -> Result<(), ConnectionError> {
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        log::debug!("Sending Packet {:?} to addr {:?}", packet, addr);
        self.socket.send_to(&buffer, addr)?;
        Ok(())
    }

    pub fn update(&mut self) {
        if let Err(e) = self.process_events() {
            log::error!("Error while processing events:\n{:?}", e);
        }
        self.update_pending_connections();
    }

    // TODO: should we remove the ConnectionError returns and do a continue?
    fn process_events(&mut self) -> Result<(), ConnectionError> {
        loop {
            let (packet, addr) = match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    let payload = &self.buffer[..len];
                    let packet = Packet::decode(payload).unwrap();
                    (packet, addr)
                }
                // Break from the loop if would block
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };

            if let Some(client_id) = self.find_client_by_addr(&addr) {
                if packet.id() == PACKET_CONNECTION {
                    log::debug!(
                        "Received Packet Connection from client {} already connected",
                        client_id.id
                    );
                    self.send_packet(Packet::ConnectionDenied, &addr)?;
                    continue;
                    //return Err(ConnectionError::ClientAlreadyConnected);
                }
            }

            if self.clients.len() >= self.config.max_clients {
                self.send_packet(Packet::ConnectionDenied, &addr)?;
                log::debug!("Connection Denied to addr {}, server is full.", addr);
                continue;
                //return Err(ConnectionError::Denied);
            }

            let connection = match self.find_connection_by_addr(&addr) {
                Some(connection) => connection,
                None => {
                    if let Packet::ConnectionRequest(client_id) = packet {
                        let new_connection = HandleConnection::new(client_id, addr.clone());
                        self.connecting.insert(client_id, new_connection);
                        self.connecting.get_mut(&client_id).unwrap()
                    } else {
                        log::debug!(
                            "Received invalid packet {} from {} before connection request",
                            packet.id(),
                            addr
                        );
                        continue;
                        //return Err(ConnectionError::InvalidPacket);
                    }
                }
            };

            connection.process_packet(&packet);
        }
    }

    fn update_pending_connections(&mut self) {
        let mut connected_connections = vec![];
        for connection in self.connecting.values() {
            match connection.state {
                ResponseConnectionState::SendingChallenge => {
                    if let Err(e) = self.send_packet(Packet::Challenge, &connection.addr) {
                        log::debug!(
                            "Error while sending Challenge Packet to {}: {:?}",
                            connection.addr,
                            e
                        );
                    }
                }
                ResponseConnectionState::SendingHeartBeat => {
                    if let Err(e) = self.send_packet(Packet::HeartBeat, &connection.addr) {
                        log::debug!(
                            "Error while sending HearBeat Packet to {}: {:?}",
                            connection.addr,
                            e
                        );
                    }
                }
                ResponseConnectionState::Connected => {
                    connected_connections.push(connection.client_id);
                }
            }
        }
        for connected in connected_connections {
            let connection = self
                .connecting
                .remove(&connected)
                .expect("Should only connect existing clients.");
            if self.clients.len() >= self.config.max_clients {
                log::debug!(
                    "Connection from {} successfuly stablished but server was full.",
                    connection.addr
                );
                self.send_packet(Packet::ConnectionDenied, &connection.addr);
                continue;
            }

            log::debug!(
                "Connection stablished with client {} ({}).",
                connection.client_id,
                connection.addr,
            );
            let client = Client::new(connection.client_id, connection.addr);
            self.clients.insert(client.id, client);
        }
    }
}
