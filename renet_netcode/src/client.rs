use std::{
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renetcode::{ClientAuthentication, DisconnectReason, NetcodeClient, NetcodeError, NETCODE_MAX_PACKET_BYTES};

use renet::{ClientId, RenetClient};

use super::NetcodeTransportError;

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
        let netcode_client = NetcodeClient::new(current_time, authentication)?;

        Ok(Self {
            buffer: [0u8; NETCODE_MAX_PACKET_BYTES],
            socket,
            netcode_client,
        })
    }

    pub fn addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn client_id(&self) -> ClientId {
        self.netcode_client.client_id()
    }

    /// Returns the duration since the client last received a packet.
    /// Usefull to detect timeouts.
    pub fn time_since_last_received_packet(&self) -> Duration {
        self.netcode_client.time_since_last_received_packet()
    }

    /// Disconnect the client from the transport layer.
    /// This sends the disconnect packet instantly, use this when closing/exiting games,
    /// should use [RenetClient::disconnect][crate::RenetClient::disconnect] otherwise.
    pub fn disconnect(&mut self) {
        if self.netcode_client.is_disconnected() {
            return;
        }

        match self.netcode_client.disconnect() {
            Ok((addr, packet)) => {
                if let Err(e) = self.socket.send_to(packet, addr) {
                    log::error!("Failed to send disconnect packet: {e}");
                }
            }
            Err(e) => log::error!("Failed to generate disconnect packet: {e}"),
        }
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
    pub fn update(&mut self, _duration: Duration, client: &mut RenetClient) -> Result<(), NetcodeTransportError> {
        if let Some(reason) = self.netcode_client.disconnect_reason() {
            // Mark the client as disconnected if an error occured in the transport layer
            client.disconnect_due_to_transport();

            return Err(NetcodeError::Disconnected(reason).into());
        }

        if let Some(error) = client.disconnect_reason() {
            let (addr, disconnect_packet) = self.netcode_client.disconnect()?;
            self.socket.send_to(disconnect_packet, addr)?;
            return Err(error.into());
        }

        if self.netcode_client.is_connected() {
            client.set_connected();
        } else if self.netcode_client.is_connecting() {
            client.set_connecting();
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
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => break,
                Err(e) => return Err(NetcodeTransportError::IO(e)),
            };

            if let Some(payload) = self.netcode_client.process_packet(packet) {
                client.process_packet(payload);
            }
        }

        if let Some((packet, addr)) = self.netcode_client.update() {
            self.socket.send_to(packet, addr)?;
        }

        Ok(())
    }
}
