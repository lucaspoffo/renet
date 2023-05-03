use std::{
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renetcode::{
    ConnectToken, DisconnectReason, NetcodeClient, NetcodeError, NETCODE_KEY_BYTES, NETCODE_MAX_PACKET_BYTES, NETCODE_USER_DATA_BYTES,
};

use crate::remote_connection::RemoteConnection;

use super::NetcodeTransportError;

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
        self.netcode_client.is_connected()
    }

    /// If the client is disconnected, returns the reason.
    pub fn disconnected(&self) -> Option<DisconnectReason> {
        self.netcode_client.disconnected()
    }

    pub fn send_packets(&mut self, connection: &mut RemoteConnection) -> Result<(), NetcodeError> {
        if let Some(reason) = self.netcode_client.disconnected() {
            return Err(NetcodeError::Disconnected(reason));
        }

        let packets = connection.get_packets_to_send();
        for packet in packets.into_iter() {
            let (addr, payload) = self.netcode_client.generate_payload_packet(&packet)?;
            self.socket.send_to(payload, addr)?;
        }

        Ok(())
    }

    pub fn update(&mut self, duration: Duration, connection: &mut RemoteConnection) -> Result<(), NetcodeTransportError> {
        if let Some(reason) = self.netcode_client.disconnected() {
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
