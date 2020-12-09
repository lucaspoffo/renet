use crate::channel::{Channel, ChannelConfig, ChannelPacketData};
use crate::endpoint::{Config, Endpoint, NetworkInfo};
use crate::error::RenetError;
use crate::client::ClientConnected;
use crate::server::{ServerConfig, Server};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use log::{debug, error};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;

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
pub enum ConnectionPacket {
    ConnectionRequest(ClientId),
    ConnectionDenied,
    Challenge,
    ChallengeResponse,
    HeartBeat,
    Payload(Box<[u8]>),
    Disconnect,
}

impl ConnectionPacket {
    pub fn id(&self) -> u8 {
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

    pub fn size(&self) -> usize {
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

    pub fn encode(&self, mut buffer: &mut [u8]) -> Result<(), ConnectionError> {
        buffer.write_u8(self.id())?;
        self.write(&mut buffer)?;
        Ok(())
    }

    pub fn decode(mut buffer: &[u8]) -> Result<ConnectionPacket, ConnectionError> {
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


#[derive(Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Accepted,
    Denied,
    SendingConnectionRequest,
    SendingChallengeResponse,
    TimedOut,
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
    pub fn new(server_addr: SocketAddr, endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            channels: HashMap::new(),
            addr: server_addr,
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn add_channel(&mut self, channel_id: u8, mut channel: Box<dyn Channel>) {
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

    pub fn process_payload(&mut self, payload: &[u8]) {
        let channel_packets = match bincode::deserialize::<Vec<ChannelPacketData>>(&payload) {
            Ok(x) => x,
            Err(e) => {
                error!(
                    "Failed to deserialize ChannelPacketData from payload: {:?}",
                    e
                );
                return;
            }
        };

        for channel_packet_data in channel_packets.iter() {
            let channel = match self.channels.get_mut(&channel_packet_data.channel_id()) {
                Some(c) => c,
                None => {
                    error!("Received channel packet with invalid id: {:?}", channel_packet_data.channel_id());
                    continue;
                }
            };
            channel.process_packet_data(channel_packet_data);
        }

        for ack in self.endpoint.get_acks().iter() {
            for channel in self.channels.values_mut() {
                channel.process_ack(*ack);
            }
        }
        self.endpoint.reset_acks();
    }

    pub fn send_payload(&mut self, payload: &[u8], socket: &UdpSocket) -> Result<(), RenetError> {
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

    pub fn get_packet(&mut self) -> Result<Option<Box<[u8]>>, RenetError> {
        let sequence = self.endpoint.sequence();
        let mut channel_packets: Vec<ChannelPacketData> = vec![];
        for channel in self.channels.values_mut() {
            let packet_data = channel.get_packet_data(
                Some(self.endpoint.config().max_packet_size as u32),
                sequence,
            );
            if let Some(packet_data) = packet_data {
                channel_packets.push(packet_data);
            }
        }
        if channel_packets.is_empty() {
            return Ok(None);
        }
        let payload = match bincode::serialize(&channel_packets) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to serialize Vec<ChannelPacketData>: {:?}", e);
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

pub enum HandleConnectionState {
    SendingChallenge,
    SendingHeartBeat,
    Accepted,
}

pub struct HandleConnection {
    addr: SocketAddr,
    client_id: ClientId,
    state: HandleConnectionState,
}

impl HandleConnection {
    pub fn new(client_id: ClientId, addr: SocketAddr) -> Self {
        Self {
            client_id,
            addr,
            state: HandleConnectionState::SendingChallenge,
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn state(&self) -> &HandleConnectionState {
        &self.state
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub fn process_packet(&mut self, packet: &ConnectionPacket) {
        match packet {
            ConnectionPacket::ConnectionRequest(_) => {}
            ConnectionPacket::ChallengeResponse => {
                if let HandleConnectionState::SendingChallenge = self.state {
                    //TODO: check if challenge is valid
                    debug!("Received Challenge Response from {}.", self.addr);
                    self.state = HandleConnectionState::SendingHeartBeat;
                }
            }
            ConnectionPacket::HeartBeat => {
                if let HandleConnectionState::SendingHeartBeat = self.state {
                    debug!(
                        "Received HeartBeat from {}, accepted connection.",
                        self.addr
                    );
                    self.state = HandleConnectionState::Accepted;
                }
            }
            _ => {}
        }
    }
}

pub type ClientId = u64;

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
}
