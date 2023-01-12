use std::{
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renetcode::{
    ConnectToken, DisconnectReason, NetcodeClient, NetcodeError, NETCODE_KEY_BYTES, NETCODE_MAX_PACKET_BYTES, NETCODE_USER_DATA_BYTES,
};

use super::{NetcodeTransportError, NUM_DISCONNECT_PACKETS_TO_SEND, ClientTransport};
use crate::{remote_connection::RemoteConnection, serialize_disconnect_packet};

/// Configuration to establishe an secure ou unsecure connection with the server.
#[allow(clippy::large_enum_variant)]
pub enum ClientAuthentication {
    /// Establishes a safe connection with the server using the [ConnectToken].
    ///
    /// See also [ServerAuthentication::Secure][crate::ServerAuthentication::Secure]
    Secure { connect_token: ConnectToken },
    /// Establishes an unsafe connection with the server, useful for testing and prototyping.
    ///
    /// See also [ServerAuthentication::Unsecure][crate::ServerAuthentication::Unsecure]
    Unsecure {
        protocol_id: u64,
        client_id: u64,
        server_addr: SocketAddr,
        user_data: Option<[u8; NETCODE_USER_DATA_BYTES]>,
    },
}

pub struct NetcodeClientTransport {
    socket: UdpSocket,
    netcode_client: NetcodeClient,
    buffer: [u8; NETCODE_MAX_PACKET_BYTES],
}

impl NetcodeClientTransport {
    pub fn new(socket: UdpSocket, current_time: Duration, authentication: ClientAuthentication) -> Result<Self, NetcodeError> {
        socket.set_nonblocking(true)?;
        let connect_token: ConnectToken = match authentication {
            ClientAuthentication::Unsecure {
                server_addr,
                protocol_id,
                client_id,
                user_data,
            } => ConnectToken::generate(
                current_time,
                protocol_id,
                300,
                client_id,
                15,
                vec![server_addr],
                user_data.as_ref(),
                &[0; NETCODE_KEY_BYTES],
            )?,
            ClientAuthentication::Secure { connect_token } => connect_token,
        };

        let netcode_client = NetcodeClient::new(current_time, connect_token);

        Ok(Self {
            buffer: [0u8; NETCODE_MAX_PACKET_BYTES],
            socket,
            netcode_client,
        })
    }

    pub fn client_id(&self) -> u64 {
        self.netcode_client.client_id()
    }

    pub fn is_connected(&self) -> bool {
        self.netcode_client.connected()
    }

    /// If the client is disconnected, returns the reason.
    pub fn disconnected(&self) -> Option<DisconnectReason> {
        self.netcode_client.disconnected()
    }

    pub fn send_packets(&mut self, connection: &mut RemoteConnection) -> Result<(), NetcodeTransportError> {
        if self.netcode_client.connected() {
            let packets = connection.get_packets_to_send()?;

            for packet in packets.into_iter() {
                let (addr, payload) = self.netcode_client.generate_payload_packet(&packet)?;
                self.socket.send_to(payload, addr)?;
            }
        }

        Ok(())
    }

    pub fn update(&mut self, duration: Duration, connection: &mut RemoteConnection) -> Result<(), NetcodeTransportError> {
        if let Some(reason) = self.netcode_client.disconnected() {
            return Err(NetcodeError::Disconnected(reason).into());
        }

        if let Some(reason) = connection.disconnected() {
            match serialize_disconnect_packet(reason) {
                Err(e) => log::error!("Failed to serialize disconnect packet: {}", e),
                Ok(packet) => match self.netcode_client.generate_payload_packet(&packet) {
                    Err(e) => log::error!("Failed to encrypt disconnect packet: {}", e),
                    Ok((addr, payload)) => {
                        for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                            self.socket.send_to(payload, addr)?;
                        }
                    }
                },
            }
            _ = self.netcode_client.disconnect();
        }

        loop {
            let packet = match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if addr != self.netcode_client.server_addr() {
                        log::debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }

                    &mut self.buffer[..len]
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(NetcodeTransportError::IO(e)),
            };

            if let Some(payload) = self.netcode_client.process_packet(packet) {
                connection.process_packet(payload)?;
            }
        }

        if let Some((packet, addr)) = self.netcode_client.update(duration) {
            self.socket.send_to(packet, addr)?;
        }

        Ok(())
    }
}

impl ClientTransport for NetcodeClientTransport {
    fn send(&mut self, packet: &[u8]) {
        match self.netcode_client.generate_payload_packet(&packet) {
            Err(e) => log::error!("Failed to encrypt payload packet: {}", e),
            Ok((addr, payload)) => {
                if let Err(e) = self.socket.send_to(payload, addr) {
                    log::error!("Failed to send packet to client {}: {}", addr, e);
                }
            }
        }
    }
}