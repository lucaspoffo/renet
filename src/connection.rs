use crate::channel::{Channel, ChannelConfig, ChannelPacketData};
use crate::endpoint::{Config, Endpoint, NetworkInfo};
use crate::error::RenetError;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use log::{debug, error};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;
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
enum ConnectionPacket {
    ConnectionRequest(ClientId),
    ConnectionDenied,
    Challenge,
    ChallengeResponse,
    HeartBeat,
    Payload(Box<[u8]>),
    Disconnect,
}

impl ConnectionPacket {
    fn id(&self) -> u8 {
        match *self {
            ConnectionPacket::ConnectionRequest(_) => PACKET_CONNECTION,
            ConnectionPacket::ConnectionDenied => PACKET_CONNECTION_DENIED,
            ConnectionPacket::Challenge => PACKET_CHALLENGE,
            ConnectionPacket::ChallengeResponse => PACKET_CHALLENGE_RESPONSE,
            ConnectionPacket::HeartBeat => PACKET_HEARTBEAT,
            ConnectionPacket::Disconnect => PACKET_DISCONNECT,
            ConnectionPacket::Payload(_) => PACKET_PAYLOAD,
        }
    }

    fn size(&self) -> usize {
        match self {
            ConnectionPacket::ConnectionRequest(_) => 9,
            ConnectionPacket::Payload(ref p) => 1 + p.len(),
            _ => 1,
        }
    }

    fn write<W>(&self, out: &mut W) -> Result<(), io::Error>
    where
        W: io::Write,
    {
        match *self {
            ConnectionPacket::ConnectionRequest(p) => out.write_u64::<BigEndian>(p),
            // Packet::Challenge(ref p) => p.write(out),
            // Packet::Response(ref p) => p.write(out),
            // Packet::KeepAlive(ref p) => p.write(out),
            ConnectionPacket::Payload(ref p) => out.write_all(p),
            // Packet::ConnectionDenied | Packet::Payload(_) | Packet::Disconnect => Ok(()),
            _ => Ok(()),
        }
    }

    fn encode(&self, mut buffer: &mut [u8]) -> Result<(), ConnectionError> {
        buffer.write_u8(self.id())?;
        self.write(&mut buffer)?;
        Ok(())
    }

    fn decode(mut buffer: &[u8]) -> Result<ConnectionPacket, ConnectionError> {
        let packet_type = buffer.read_u8()?;

        match packet_type {
            PACKET_CONNECTION => {
                let client_id = buffer.read_u64::<BigEndian>()?;
                Ok(ConnectionPacket::ConnectionRequest(client_id))
            }
            PACKET_PAYLOAD => {
                let payload = buffer[..buffer.len()].to_vec().into_boxed_slice();
                Ok(ConnectionPacket::Payload(payload))
            }
            PACKET_DISCONNECT => Ok(ConnectionPacket::Disconnect),
            PACKET_HEARTBEAT => Ok(ConnectionPacket::HeartBeat),
            PACKET_CHALLENGE => Ok(ConnectionPacket::Challenge),
            PACKET_CONNECTION_DENIED => Ok(ConnectionPacket::ConnectionDenied),
            PACKET_CHALLENGE_RESPONSE => Ok(ConnectionPacket::ChallengeResponse),
            _ => Err(ConnectionError::InvalidPacket),
        }
    }
}

// TODO separate connected state from connecting state
// TODO investigate if we can separate the client state
// in the server from the from the client itself
#[derive(Debug, Eq, PartialEq)]
pub enum ClientState {
    Connected,
    Disconnected,
    ConnectionTimedOut,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Accepted,
    Denied,
    SendingConnectionRequest,
    SendingChallengeResponse,
    TimedOut,
    ResponseTimedOut,
    RequestTimedOut
}

pub struct RequestConnection {
    state: ConnectionState,
    socket: UdpSocket,
    server_addr: SocketAddr,
    id: ClientId,
}

#[derive(Debug)]
pub enum ConnectionError {
    Denied,
    IOError(io::Error),
    InvalidPacket,
    ClientAlreadyConnected,
    ClientDisconnected,
}

impl From<io::Error> for ConnectionError {
    fn from(inner: io::Error) -> ConnectionError {
        ConnectionError::IOError(inner)
    }
}

pub struct Connection {
    pub endpoint: Endpoint,
    channels: HashMap<u8, Box<dyn Channel>>,
    addr: SocketAddr,
}

impl Connection {
    fn new(server_addr: SocketAddr, endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            channels: HashMap::new(),
            addr: server_addr,
        }
    }

    fn add_channel(&mut self, channel_id: u8, mut channel: Box<dyn Channel>) {
        channel.set_id(channel_id);
        self.channels.insert(channel_id, channel);
    }

    pub fn update_channels_current_time(&mut self, current_time: Instant) {
        for channel in self.channels.values_mut() {
            channel.update_current_time(current_time.clone());
        }
    }

    pub fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) {
        let channel = self
            .channels
            .get_mut(&channel_id)
            .expect("Sending message to invalid channel");
        channel.send_message(message);
    }

    fn process_payload(&mut self, payload: &[u8]) {
        let channel_packets = match bincode::deserialize::<Vec<(u8, ChannelPacketData)>>(&payload) {
            Ok(x) => x,
            Err(e) => {
                error!(
                    "Failed to deserialize ChannelPacketData from payload: {:?}",
                    e
                );
                return;
            }
        };

        for (channel_id, packet_data) in channel_packets.iter() {
            let channel = match self.channels.get_mut(channel_id) {
                Some(c) => c,
                None => {
                    error!("Received channel packet with invalid id: {:?}", channel_id);
                    continue;
                }
            };
            channel.process_packet_data(packet_data);
        }

        for ack in self.endpoint.get_acks().iter() {
            for channel in self.channels.values_mut() {
                channel.process_ack(*ack);
            }
        }
        self.endpoint.reset_acks();
    }

    fn send_payload(&mut self, payload: &[u8], socket: &UdpSocket) -> Result<(), RenetError> {
        let mut reliable_packets = self.endpoint.generate_packets(payload)?;
        for reliable_packet in reliable_packets.iter_mut() {
            // TODO remove clone here
            let payload_packet =
                ConnectionPacket::Payload(reliable_packet.clone().into_boxed_slice());
            let mut buffer = vec![0u8; payload_packet.size()];
            payload_packet.encode(&mut buffer)?;
            socket.send_to(&buffer, self.addr)?;
        }
        Ok(())
    }

    fn get_packet(&mut self) -> Result<Option<Box<[u8]>>, RenetError> {
        let sequence = self.endpoint.sequence();
        let mut channel_packets: Vec<(u8, ChannelPacketData)> = vec![];
        for (id, channel) in self.channels.iter_mut() {
            let packet_data = channel.get_packet_data(
                Some(self.endpoint.config().max_packet_size as u32),
                sequence,
            );
            if let Some(packet_data) = packet_data {
                channel_packets.push((id.clone(), packet_data));
            }
        }
        if channel_packets.is_empty() {
            return Ok(None);
        }
        let payload = match bincode::serialize(&channel_packets) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to serialize Vec<(u8, ChannelPacketData)>: {:?}", e);
                return Err(RenetError::SerializationFailed);
            }
        };

        Ok(Some(payload.into_boxed_slice()))
    }

    pub fn receive_message_from_channel(&mut self, channel_id: u8) -> Option<Box<[u8]>> {
        let channel = match self.channels.get_mut(&channel_id) {
            Some(c) => c,
            None => {
                error!(
                    "Tried to receive message from invalid channel {}.",
                    channel_id
                );
                return None;
            }
        };

        return channel.receive_message();
    }

    pub fn receive_all_messages_from_channel(&mut self, channel_id: u8) -> Vec<Box<[u8]>> {
        let mut messages = vec![];
        let channel = match self.channels.get_mut(&channel_id) {
            Some(c) => c,
            None => {
                error!(
                    "Tried to receive message from invalid channel {}.",
                    channel_id
                );
                return messages;
            }
        };

        while let Some(message) = channel.receive_message() {
            messages.push(message);
        }

        messages
    }
}

// TODO: Move channels and endpoit to it's own struct
// The ClientConnected and Client use the same logic with them
pub struct ClientConnected {
    socket: UdpSocket,
    id: ClientId,
    connection: Connection,
}

impl ClientConnected {
    fn new(id: ClientId, socket: UdpSocket, server_addr: SocketAddr, endpoint: Endpoint) -> Self {
        Self {
            id,
            socket,
            connection: Connection::new(server_addr, endpoint),
        }
    }

    pub fn add_channel(&mut self, channel_id: u8, channel: Box<dyn Channel>) {
        self.connection.add_channel(channel_id, channel);
    }

    pub fn id(&self) -> ClientId {
        self.id
    }

    pub fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) {
        self.connection.send_message(channel_id, message);
    }

    pub fn receive_all_messages_from_channel(&mut self, channel_id: u8) -> Vec<Box<[u8]>> {
        self.connection
            .receive_all_messages_from_channel(channel_id)
    }

    pub fn network_info(&self) -> &NetworkInfo {
        self.connection.endpoint.network_info()
    }

    pub fn update_network_info(&mut self) -> &NetworkInfo {
        self.connection.endpoint.update_sent_bandwidth();
        self.connection.endpoint.update_received_bandwidth();
        self.connection.endpoint.network_info()
    }

    pub fn send_packets(&mut self) -> Result<(), RenetError> {
        if let Some(payload) = self.connection.get_packet()? {
            self.connection.send_payload(&payload, &self.socket)?;
        }
        Ok(())
    }

    fn process_packet(&mut self, mut packet: ConnectionPacket) {
        match packet {
            ConnectionPacket::Payload(ref mut payload) => {
                match self.connection.endpoint.process_payload(payload) {
                    Ok(Some(payload)) => {
                        self.connection.process_payload(&payload);
                    }
                    Err(e) => {
                        error!(
                            "Error in endpoint from server while processing payload:\n{:?}",
                            e
                        );
                    }
                    Ok(None) => {}
                }
            }
            ConnectionPacket::HeartBeat => {
                debug!("Received HeartBeat from the server");
            }
            _ => {
                debug!(
                    "Ignoring Packet type {} while in connected state",
                    packet.id()
                );
            }
        }
    }

    pub fn process_events(&mut self, current_time: Instant) -> Result<(), RenetError> {
        let mut buffer = vec![0u8; 1500];
        self.connection.update_channels_current_time(current_time);
        loop {
            let packet = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == self.connection.addr {
                        let payload = &buffer[..len];
                        ConnectionPacket::decode(payload)?
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };
            self.process_packet(packet);
        }
    }
}

impl RequestConnection {
    pub fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
    ) -> Result<Self, ConnectionError> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            id,
            socket,
            server_addr,
            state: ConnectionState::SendingConnectionRequest,
        })
    }

    fn send_packet(&self, packet: ConnectionPacket) -> Result<(), ConnectionError> {
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        debug!("Send Packet buffer: {:?}", buffer);
        self.socket.send_to(&buffer, self.server_addr)?;
        Ok(())
    }

    fn process_packet(&mut self, packet: ConnectionPacket) -> Result<(), ConnectionError> {
        match packet {
            ConnectionPacket::Challenge => {
                if self.state == ConnectionState::SendingConnectionRequest {
                    debug!("Received Challenge Packet, moving to State Sending Response");
                    self.state = ConnectionState::SendingChallengeResponse;
                }
            }
            ConnectionPacket::HeartBeat => {
                if self.state == ConnectionState::SendingChallengeResponse {
                    debug!("Received HeartBeat while sending challenge response, successfuly connected");
                    self.state = ConnectionState::Accepted;
                }
            }
            ConnectionPacket::ConnectionDenied => {
                self.state = ConnectionState::Denied;
                return Err(ConnectionError::Denied);
            }
            p => {
                debug!("Received invalid packet {:?}", p);
            }
        }
        Ok(())
    }

    pub fn update(&mut self) -> Result<Option<ClientConnected>, ConnectionError> {
        self.process_events()?;
        debug!("State: {:?}", self.state);
        match self.state {
            ConnectionState::SendingConnectionRequest => {
                self.send_packet(ConnectionPacket::ConnectionRequest(self.id))?;
            }
            ConnectionState::SendingChallengeResponse => {
                self.send_packet(ConnectionPacket::ChallengeResponse)?;
            }
            ConnectionState::Accepted => {
                self.send_packet(ConnectionPacket::HeartBeat)?;
                let config = Config::default();
                let endpoint = Endpoint::new(config);
                return Ok(Some(ClientConnected::new(
                    self.id,
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
        // TODO: pass this buffer to struct
        let mut buffer = vec![0u8; 16000];
        loop {
            let packet = match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        let payload = &buffer[..len];
                        let packet = ConnectionPacket::decode(payload)?;
                        packet
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };

            self.process_packet(packet)?;
        }
    }
}

enum ResponseConnectionState {
    SendingChallenge,
    SendingHeartBeat,
    Accepted,
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

    fn process_packet(&mut self, packet: &ConnectionPacket) {
        match packet {
            ConnectionPacket::ConnectionRequest(_) => {}
            ConnectionPacket::ChallengeResponse => {
                if let ResponseConnectionState::SendingChallenge = self.state {
                    //TODO: check if challenge is valid
                    debug!("Received Challenge Response from {}.", self.addr);
                    self.state = ResponseConnectionState::SendingHeartBeat;
                }
            }
            ConnectionPacket::HeartBeat => {
                if let ResponseConnectionState::SendingHeartBeat = self.state {
                    debug!(
                        "Received HeartBeat from {}, accepted connection.",
                        self.addr
                    );
                    self.state = ResponseConnectionState::Accepted;
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
    connection: Connection,
}

impl Client {
    fn new(id: ClientId, addr: SocketAddr, endpoint: Endpoint) -> Self {
        Self {
            id,
            state: ClientState::Connected,
            connection: Connection::new(addr, endpoint),
        }
    }

    fn is_connected(&self) -> bool {
        self.state == ClientState::Connected
    }

    fn process_packet(&mut self, packet: &ConnectionPacket) {
        match packet {
            ConnectionPacket::Payload(payload) => {
                match self.connection.endpoint.process_payload(&payload) {
                    Ok(Some(payload)) => {
                        self.connection.process_payload(&payload);
                    }
                    Err(e) => {
                        error!(
                            "Error in endpoint from server while processing payload:\n{:?}",
                            e
                        );
                    }
                    Ok(None) => {}
                }
            }
            ConnectionPacket::ConnectionRequest(_) => {
                debug!(
                    "Received Packet Connection from client {} already connected",
                    self.id
                );
            }
            ConnectionPacket::HeartBeat => {
                debug!("Received HeartBeat from the server");
            }
            _ => {
                debug!(
                    "Ignoring Packet type {} while in connected state",
                    packet.id()
                );
            }
        }
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
    channels_config: HashMap<u8, ChannelConfig>,
    current_time: Instant,
}

// TODO we should use a Sender / Receiver from something like crossbeam
// to dispatch these events
// enum ServerEvent {
//     ClientConnected(ClientId),
//     ClientDisconnected(ClientId),
//     ServerSlotsFull,
//     RejectClient(ClientId),
// }

impl Server {
    pub fn new(socket: UdpSocket, config: ServerConfig) -> Result<Self, ConnectionError> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            clients: HashMap::new(),
            connecting: HashMap::new(),
            config,
            channels_config: HashMap::new(),
            current_time: Instant::now(),
        })
    }

    pub fn add_channel_config(&mut self, channel_id: u8, config: ChannelConfig) {
        self.channels_config.insert(channel_id, config);
    }

    pub fn has_clients(&self) -> bool {
        !self.clients.is_empty()
    }

    fn find_client_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut Client> {
        self.clients
            .values_mut()
            .find(|c| c.connection.addr == *addr)
    }

    fn find_connection_by_addr(&mut self, addr: &SocketAddr) -> Option<&mut HandleConnection> {
        self.connecting.values_mut().find(|c| c.addr == *addr)
    }

    pub fn get_client_network_info(&mut self, client_id: ClientId) -> Option<&NetworkInfo> {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.connection.endpoint.update_sent_bandwidth();
            client.connection.endpoint.update_received_bandwidth();
            return Some(client.connection.endpoint.network_info());
        }
        None
    }

    pub fn send_message_to_all_clients(&mut self, channel_id: u8, message: Box<[u8]>) {
        for client in self.clients.values_mut() {
            client.connection.send_message(channel_id, message.clone());
        }
    }

    pub fn send_message_to_client(
        &mut self,
        client_id: ClientId,
        channel_id: u8,
        message: Box<[u8]>,
    ) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.connection.send_message(channel_id, message);
        }
    }

    fn send_connection_packet(
        &self,
        packet: ConnectionPacket,
        addr: &SocketAddr,
    ) -> Result<(), ConnectionError> {
        let mut buffer = vec![0u8; packet.size()];
        packet.encode(&mut buffer)?;
        debug!("Sending Connection Packet {:?} to addr {:?}", packet, addr);
        self.socket.send_to(&buffer, addr)?;
        Ok(())
    }

    pub fn update(&mut self, current_time: Instant) {
        if let Err(e) = self.process_events(current_time) {
            error!("Error while processing events:\n{:?}", e);
        }
        self.update_pending_connections();
    }

    pub fn send_packets(&mut self) {
        for client in self.clients.values_mut() {
            match client.connection.get_packet() {
                Ok(Some(payload)) => {
                    if let Err(e) = client.connection.send_payload(&payload, &self.socket) {
                        error!("Failed to send payload for client {}: {:?}", client.id, e);
                    }
                }
                Ok(None) => {}
                Err(_) => error!("Failed to get packet for client {}.", client.id),
            }
        }
    }

    fn process_packet_from(
        &mut self,
        packet: ConnectionPacket,
        addr: &SocketAddr,
    ) -> Result<(), RenetError> {
        if let Some(client) = self.find_client_by_addr(addr) {
            client.process_packet(&packet);
            return Ok(());
        }

        if self.clients.len() >= self.config.max_clients {
            self.send_connection_packet(ConnectionPacket::ConnectionDenied, &addr)?;
            debug!("Connection Denied to addr {}, server is full.", addr);
            return Err(ConnectionError::Denied.into());
        }

        let connection = match self.find_connection_by_addr(addr) {
            Some(connection) => connection,
            None => {
                if let ConnectionPacket::ConnectionRequest(client_id) = packet {
                    let new_connection = HandleConnection::new(client_id, addr.clone());
                    self.connecting.insert(client_id, new_connection);
                    self.connecting.get_mut(&client_id).unwrap()
                } else {
                    debug!(
                        "Received invalid packet {} from {} before connection request",
                        packet.id(),
                        addr
                    );
                    return Err(ConnectionError::InvalidPacket.into());
                }
            }
        };

        connection.process_packet(&packet);
        Ok(())
    }

    // TODO: should we remove the ConnectionError returns and do a continue?
    fn process_events(&mut self, current_time: Instant) -> Result<(), RenetError> {
        for client in self.clients.values_mut() {
            client.connection.update_channels_current_time(current_time);
        }
        let mut buffer = vec![0u8; self.config.max_payload_size];
        loop {
            match self.socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    let payload = &buffer[..len];
                    let packet = match ConnectionPacket::decode(&payload) {
                        Ok(packet) => packet,
                        Err(e) => {
                            error!("Failed to decote packet:\n{:?}", e);
                            continue;
                        }
                    };
                    if let Err(e) = self.process_packet_from(packet, &addr) {
                        error!("Error while processing events:\n{:?}", e);
                    }
                }
                // Break from the loop if would block
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(e) => return Err(ConnectionError::IOError(e).into()),
            };
        }
    }

    fn update_pending_connections(&mut self) {
        let mut connected_connections = vec![];
        for connection in self.connecting.values() {
            match connection.state {
                ResponseConnectionState::SendingChallenge => {
                    if let Err(e) =
                        self.send_connection_packet(ConnectionPacket::Challenge, &connection.addr)
                    {
                        error!(
                            "Error while sending Challenge Packet to {}: {:?}",
                            connection.addr, e
                        );
                    }
                }
                ResponseConnectionState::SendingHeartBeat => {
                    if let Err(e) =
                        self.send_connection_packet(ConnectionPacket::HeartBeat, &connection.addr)
                    {
                        error!(
                            "Error while sending HearBeat Packet to {}: {:?}",
                            connection.addr, e
                        );
                    }
                }
                ResponseConnectionState::Accepted => {
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
                debug!(
                    "Connection from {} successfuly stablished but server was full.",
                    connection.addr
                );
                if let Err(e) = self
                    .send_connection_packet(ConnectionPacket::ConnectionDenied, &connection.addr)
                {
                    error!(
                        "Error while sending Connection Denied Packet to {}: {:?}",
                        connection.addr, e
                    );
                }
                continue;
            }

            debug!(
                "Connection stablished with client {} ({}).",
                connection.client_id, connection.addr,
            );
            let endpoint_config = Config::default();
            let endpoint: Endpoint = Endpoint::new(endpoint_config);
            let mut client = Client::new(connection.client_id, connection.addr, endpoint);
            for (channel_id, channel_config) in self.channels_config.iter() {
                let channel = channel_config.new_channel(self.current_time);
                client.connection.add_channel(*channel_id, channel);
            }
            self.clients.insert(client.id, client);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, UdpSocket};

    #[test]
    fn encode_decode_payload() {
        let payload = vec![1, 2, 3, 4, 5].into_boxed_slice();
        let packet = ConnectionPacket::Payload(payload);
        let mut buffer = vec![0u8; packet.size()];

        packet.encode(&mut buffer).unwrap();
        let decoded_packet = ConnectionPacket::decode(&buffer).unwrap();

        assert_eq!(packet, decoded_packet);
    }

    #[test]
    fn encode_decode_connection() {
        let packet = ConnectionPacket::ConnectionRequest(1);
        let mut buffer = vec![0u8; packet.size()];

        packet.encode(&mut buffer).unwrap();
        let decoded_packet = ConnectionPacket::decode(&buffer).unwrap();

        assert_eq!(packet, decoded_packet);
    }

    #[test]
    fn request_connection_flow() {
        let socket = UdpSocket::bind("127.0.0.1:8081").unwrap();
        let server_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut request_connection = RequestConnection::new(0, socket, server_addr).unwrap();

        assert_eq!(
            ConnectionState::SendingConnectionRequest,
            request_connection.state
        );

        request_connection
            .process_packet(ConnectionPacket::Challenge)
            .unwrap();
        assert_eq!(
            ConnectionState::SendingChallengeResponse,
            request_connection.state
        );

        request_connection
            .process_packet(ConnectionPacket::HeartBeat)
            .unwrap();
        assert_eq!(ConnectionState::Accepted, request_connection.state);
    }

    #[test]
    fn server_client_connecting_flow() {
        let socket = UdpSocket::bind("127.0.0.1:8080").unwrap();
        let server_config = ServerConfig::default();
        let mut server = Server::new(socket, server_config).unwrap();

        let client_addr: SocketAddr = "127.0.0.1:8081".parse().unwrap();
        let packet = ConnectionPacket::ConnectionRequest(0);
        server.process_packet_from(packet, &client_addr).unwrap();
        assert_eq!(server.connecting.len(), 1);

        let packet = ConnectionPacket::ChallengeResponse;
        server.process_packet_from(packet, &client_addr).unwrap();
        server.update_pending_connections();
        assert_eq!(server.connecting.len(), 1);
        assert_eq!(server.clients.len(), 0);

        let packet = ConnectionPacket::HeartBeat;
        server.process_packet_from(packet, &client_addr).unwrap();
        server.update_pending_connections();
        assert_eq!(server.connecting.len(), 0);
        assert_eq!(server.clients.len(), 1);
        assert!(server.clients.contains_key(&0));
    }
}
