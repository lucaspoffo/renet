use crate::error::RenetError;
use crate::{Config, Endpoint};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::io::{self, Cursor, Write};
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

// TODO When successfuly connected an reliable endpoint should be created
// When receiving a payload packet we should pass it to the correct endpoint,
// in ther server to the matching client

const PACKET_CONNECTION: u8 = 0;
const PACKET_CONNECTION_DENIED: u8 = 1;
const PACKET_CHALLENGE: u8 = 2;
const PACKET_CHALLENGE_RESPONSE: u8 = 3;
const PACKET_HEARTBEAT: u8 = 4;
const PACKET_PAYLOAD: u8 = 5;
const PACKET_DISCONNECT: u8 = 6;

// TODO Should we divide the packet types in 2 enum?
// One for the client packets and another for the server packets
#[derive(Debug, Eq, PartialEq)]
enum Packet {
    ConnectionRequest(ClientId),
    ConnectionDenied,
    Challenge,
    ChallengeResponse,
    HeartBeat,
    Payload(Box<[u8]>),
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
            Packet::Payload(_) => PACKET_PAYLOAD,
        }
    }

    fn size(&self) -> usize {
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
            // Packet::Challenge(ref p) => p.write(out),
            // Packet::Response(ref p) => p.write(out),
            // Packet::KeepAlive(ref p) => p.write(out),
            Packet::Payload(ref p) => out.write_all(p),
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

        match packet_type {
            PACKET_CONNECTION => {
                let client_id = buffer.read_u64::<BigEndian>()?;
                Ok(Packet::ConnectionRequest(client_id))
            }
            PACKET_PAYLOAD => {
                let payload = buffer[..buffer.len()].to_vec().into_boxed_slice();
                Ok(Packet::Payload(payload))
            }
            PACKET_DISCONNECT => Ok(Packet::Disconnect),
            PACKET_HEARTBEAT => Ok(Packet::HeartBeat),
            PACKET_CHALLENGE => Ok(Packet::Challenge),
            PACKET_CONNECTION_DENIED => Ok(Packet::ConnectionDenied),
            PACKET_CHALLENGE_RESPONSE => Ok(Packet::ChallengeResponse),
            _ => Err(ConnectionError::InvalidPacket),
        }
    }
}

// TODO separate connected state from connecting state
// TODO investigate if we can separate the client state
// in the server from the from the client itself
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

pub struct ServerConnection {
    socket: UdpSocket,
    endpoint: Endpoint,
    server_addr: SocketAddr,
    pub received_payloads: Vec<Vec<u8>>,
}

impl ServerConnection {
    fn new(socket: UdpSocket, server_addr: SocketAddr, endpoint: Endpoint) -> Self {
        Self {
            socket,
            server_addr,
            endpoint,
            received_payloads: vec![],
        }
    }

    pub fn send_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        let mut reliable_packets = self.endpoint.generate_packets(payload)?;
        for reliable_packet in reliable_packets.iter_mut() {
            // TODO remove clone here
            let payload_packet = Packet::Payload(reliable_packet.clone().into_boxed_slice());
            let mut buffer = vec![0u8; payload_packet.size()]; 
            payload_packet.encode(&mut buffer)?;
            self.socket.send_to(&buffer, self.server_addr)?;
        }
        Ok(())
    }

    pub fn process_events(&mut self) -> Result<(), RenetError> {
        let mut buffer = vec![0u8; 1500];
        loop {
            let mut packet = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        let payload = &buffer[..len];
                        Packet::decode(payload)?
                    } else {
                        log::debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };

            match packet {
                Packet::Payload(ref mut payload) => match self.endpoint.process_payload(payload) {
                    Ok(Some(payload)) => {
                        self.received_payloads.push(payload);
                    }
                    Err(e) => {
                        log::error!(
                            "Error in endpoint from server while processing payload:\n{:?}",
                            e
                        );
                    }
                    Ok(None) => {}
                },
                Packet::HeartBeat => {
                    log::debug!("Received HeartBeat from the server");
                }
                _ => {
                    log::debug!(
                        "Ignoring Packet type {} while in connected state",
                        packet.id()
                    );
                }
            }
        }
    }
}

impl RequestConnection {
    pub fn new(socket: UdpSocket, server_addr: SocketAddr) -> Result<Self, ConnectionError> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            server_addr,
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
            }
            ClientState::SendingChallengeResponse => {
                self.send_packet(Packet::ChallengeResponse)?;
            }
            ClientState::Connected => {
                self.send_packet(Packet::HeartBeat)?;
                let config = Config::default();
                let endpoint = Endpoint::new(config);
                return Ok(Some(ServerConnection::new(
                    self.socket.try_clone()?,
                    self.server_addr,
                    endpoint,
                )));
            }
            _ => {}
        }
        Ok(None)
    }

    fn process_events(&mut self) -> Result<(), ConnectionError> {
        let mut buffer = vec![0u8; 16000];
        loop {
            let packet = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        let payload = &buffer[..len];
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
    endpoint: Endpoint,
    state: ClientState,
    addr: SocketAddr,
}

impl Client {
    fn new(id: ClientId, addr: SocketAddr, endpoint: Endpoint) -> Self {
        Self {
            id,
            addr,
            endpoint,
            state: ClientState::Connected,
        }
    }

    fn send_to(&mut self, socket: &UdpSocket, payload: &[u8]) -> Result<(), RenetError> {
        self.endpoint.send_to(payload, self.addr, socket)?;
        Ok(())
    }

    fn send_payload(&mut self, socket: &UdpSocket, payload: &[u8]) -> Result<(), RenetError> {
        let mut reliable_packets = self.endpoint.generate_packets(payload)?;
        for reliable_packet in reliable_packets.iter_mut() {
            // TODO remove clone here
            let payload_packet = Packet::Payload(reliable_packet.clone().into_boxed_slice());
            let mut buffer = vec![0u8; payload_packet.size()];
            payload_packet.encode(&mut buffer)?;
            socket.send_to(&buffer, self.addr)?;
        }
        Ok(())
    }
}

pub struct ServerConfig {
    max_clients: usize,
    max_payload_size: usize,
}

impl ServerConfig {
    pub fn new(max_clients: usize, max_payload_size: usize) -> Self {
        Self {
            max_clients,
            max_payload_size,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_clients: 16,
            max_payload_size: 8 * 1024,
        }
    }
}

// TODO: add internal buffer?
pub struct Server {
    config: ServerConfig,
    socket: UdpSocket,
    clients: HashMap<ClientId, Client>,
    connecting: HashMap<ClientId, HandleConnection>,
    pub received_payloads: Vec<(ClientId, Box<[u8]>)>,
}

// TODO we should use a Sender / Receiver from something like crossbeam
// to dispatch these events
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
            received_payloads: vec![],
        })
    }

    fn find_client_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut Client> {
        self.clients.values_mut().find(|c| c.addr == *addr)
    }

    fn find_connection_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut HandleConnection> {
        self.connecting.values_mut().find(|c| c.addr == *addr)
    }

    fn send_connection_packet(
        &self,
        packet: Packet,
        addr: &SocketAddr,
    ) -> Result<(), ConnectionError> {
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        log::debug!("Sending Connection Packet {:?} to addr {:?}", packet, addr);
        self.socket.send_to(&buffer, addr)?;
        Ok(())
    }

    fn send_payload_to_clients(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        for (id, client) in self.clients.iter_mut() {
            client.send_payload(&self.socket, payload)?;
        }
        Ok(())
    }

    pub fn update(&mut self) {
        if let Err(e) = self.process_events() {
            log::error!("Error while processing events:\n{:?}", e);
        }
        self.update_pending_connections();
    }

    // TODO: should we remove the ConnectionError returns and do a continue?
    fn process_events(&mut self) -> Result<(), RenetError> {
        let mut buffer = vec![0u8; self.config.max_payload_size];
        let mut received_payloads: Vec<(ClientId, Box<[u8]>)> = vec![];
        loop {
            let (packet, addr) = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    let payload = &buffer[..len];
                    let packet = match Packet::decode(&payload) {
                        Ok(packet) => packet,
                        Err(e) => {
                            log::error!("Failed to decote packet:\n{:?}", e);
                            continue;
                        }
                    };

                    if let Some(client) = self.find_client_by_addr(&addr) {
                        // TODO add this to the Client struct
                        match packet {
                            Packet::Payload(payload) => {
                                match client.endpoint.process_payload(&payload) {
                                    Ok(Some(payload)) => {
                                        received_payloads.push((client.id, payload.into()));
                                        continue;
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Error in endpoint from server while processing payload:\n{:?}",
                                            e
                                        );
                                        continue;
                                    }
                                    Ok(None) => {
                                        continue;
                                    }
                                }
                            }
                            Packet::ConnectionRequest(_) => {
                                log::debug!(
                                    "Received Packet Connection from client {} already connected",
                                    client.id
                                );
                                continue;
                            }
                            Packet::HeartBeat => {
                                log::debug!("Received HeartBeat from the server");
                                continue;
                            }
                            _ => {
                                log::debug!(
                                    "Ignoring Packet type {} while in connected state",
                                    packet.id()
                                );
                                continue;
                            }
                        }
                    } else {
                        (packet, addr)
                    }
                }
                // Break from the loop if would block
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // TODO do something else here
                    for p in received_payloads {
                        self.received_payloads.push(p);
                    }

                    return Ok(());
                }
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };

            if self.clients.len() >= self.config.max_clients {
                self.send_connection_packet(Packet::ConnectionDenied, &addr)?;
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
                    if let Err(e) = self.send_connection_packet(Packet::Challenge, &connection.addr)
                    {
                        log::debug!(
                            "Error while sending Challenge Packet to {}: {:?}",
                            connection.addr,
                            e
                        );
                    }
                }
                ResponseConnectionState::SendingHeartBeat => {
                    if let Err(e) = self.send_connection_packet(Packet::HeartBeat, &connection.addr)
                    {
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
                self.send_connection_packet(Packet::ConnectionDenied, &connection.addr);
                continue;
            }

            log::debug!(
                "Connection stablished with client {} ({}).",
                connection.client_id,
                connection.addr,
            );
            let endpoint_config = Config::default();
            let endpoint: Endpoint = Endpoint::new(endpoint_config);
            let client = Client::new(connection.client_id, connection.addr, endpoint);
            self.clients.insert(client.id, client);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_payload() {
        let payload = vec![1, 2, 3, 4, 5].into_boxed_slice();
        let packet = Packet::Payload(payload);
        let mut buffer = vec![0u8; packet.size()];

        packet.encode(&mut buffer).unwrap();
        let decoded_packet = Packet::decode(&buffer).unwrap();
        
        assert_eq!(packet, decoded_packet);
    }

    #[test]
    fn encode_decode_connection() {
        let packet = Packet::ConnectionRequest(1);
        let mut buffer = vec![0u8; packet.size()];

        packet.encode(&mut buffer).unwrap();
        let decoded_packet = Packet::decode(&buffer).unwrap();
        
        assert_eq!(packet, decoded_packet);
    }
}
