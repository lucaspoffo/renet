use std::{
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renetcode::{
    ConnectToken, DisconnectReason, NetcodeClient, NetcodeError, NETCODE_KEY_BYTES, NETCODE_MAX_PACKET_BYTES, NETCODE_USER_DATA_BYTES,
};

use crate::remote_connection::RenetClient;

use super::NetcodeTransportError;

/// Configuration to establish an secure ou unsecure connection with the server.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ClientAuthentication {
    /// Establishes a safe connection with the server using the [crate::transport::ConnectToken].
    ///
    /// See also [crate::transport::ServerAuthentication::Secure]
    Secure { connect_token: ConnectToken },
    /// Establishes an unsafe connection with the server, useful for testing and prototyping.
    ///
    /// See also [crate::transport::ServerAuthentication::Unsecure]
    Unsecure {
        protocol_id: u64,
        client_id: u64,
        server_addr: SocketAddr,
        user_data: Option<[u8; NETCODE_USER_DATA_BYTES]>,
    },
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct NetcodeClientTransport {
    socket: UdpSocket,
    netcode_client: NetcodeClient,
    buffer: [u8; NETCODE_MAX_PACKET_BYTES],
}

impl NetcodeClientTransport {
    pub fn new(current_time: Duration, authentication: ClientAuthentication, socket: UdpSocket) -> Result<Self, NetcodeError> {
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

    pub fn is_connecting(&self) -> bool {
        self.netcode_client.is_connecting()
    }

    pub fn is_connected(&self) -> bool {
        self.netcode_client.is_connected()
    }

    pub fn is_disconnected(&self) -> bool {
        self.netcode_client.is_disconnected()
    }

    /// Returns the duration since the client last received a packet.
    /// Usefull to detect timeouts.
    pub fn time_since_last_received_packet(&self) -> Duration {
        self.netcode_client.time_since_last_received_packet()
    }

    /// If the client is disconnected, returns the reason.
    pub fn disconnect_reason(&self) -> Option<DisconnectReason> {
        self.netcode_client.disconnect_reason()
    }

    /// Send packets to the server.
    /// Should be called every tick
    pub fn send_packets(&mut self, connection: &mut RenetClient) -> Result<(), NetcodeTransportError> {
        if let Some(reason) = self.netcode_client.disconnect_reason() {
            return Err(NetcodeError::Disconnected(reason).into());
        }

        let packets = connection.get_packets_to_send();
        for packet in packets {
            let (addr, payload) = self.netcode_client.generate_payload_packet(&packet)?;
            self.socket.send_to(payload, addr)?;
        }

        Ok(())
    }

    /// Advances the transport by the duration, and receive packets from the network.
    pub fn update(&mut self, duration: Duration, connection: &mut RenetClient) -> Result<(), NetcodeTransportError> {
        if let Some(reason) = self.netcode_client.disconnect_reason() {
            return Err(NetcodeError::Disconnected(reason).into());
        }

        if let Some(error) = connection.disconnect_reason() {
            let (addr, disconnect_packet) = self.netcode_client.disconnect()?;
            self.socket.send_to(disconnect_packet, addr)?;
            return Err(error.into());
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
                connection.process_packet(payload);
            }
        }

        if let Some((packet, addr)) = self.netcode_client.update(duration) {
            self.socket.send_to(packet, addr)?;
        }

        Ok(())
    }
}
