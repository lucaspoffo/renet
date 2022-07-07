use crate::{
    error::{DisconnectionReason, RenetError},
    network_info::{ClientPacketInfo, NetworkInfo, PacketInfo},
    RenetConnectionConfig, NUM_DISCONNECT_PACKETS_TO_SEND,
};

use log::debug;
use rechannel::{error::RechannelError, remote_connection::RemoteConnection, Bytes};
use renetcode::{ConnectToken, NetcodeClient, NetcodeError, NETCODE_KEY_BYTES, NETCODE_MAX_PACKET_BYTES};

use std::net::UdpSocket;
use std::time::Duration;
use std::{io, net::SocketAddr};

/// A client that establishes an authenticated connection with a server.
/// Can send/receive encrypted messages from/to the server.
pub struct RenetClient {
    current_time: Duration,
    netcode_client: NetcodeClient,
    socket: UdpSocket,
    reliable_connection: RemoteConnection,
    buffer: [u8; NETCODE_MAX_PACKET_BYTES],
    client_packet_info: ClientPacketInfo,
}

impl RenetClient {
    pub fn new(
        current_time: Duration,
        socket: UdpSocket,
        client_id: u64,
        connect_token: ConnectToken,
        config: RenetConnectionConfig,
    ) -> Result<Self, std::io::Error> {
        socket.set_nonblocking(true)?;
        let reliable_connection = RemoteConnection::new(current_time, config.to_connection_config());
        let netcode_client = NetcodeClient::new(current_time, client_id, connect_token);
        let client_packet_info = ClientPacketInfo::new(config.bandwidth_smoothing_factor);

        Ok(Self {
            current_time,
            buffer: [0u8; NETCODE_MAX_PACKET_BYTES],
            socket,
            reliable_connection,
            netcode_client,
            client_packet_info,
        })
    }

    #[doc(hidden)]
    pub fn __test() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let server_addr = "127.0.0.1:5000".parse().unwrap();
        let connect_token =
            ConnectToken::generate(Duration::ZERO, 0, 300, 0, 15, vec![server_addr], None, &[0; NETCODE_KEY_BYTES]).unwrap();
        Self::new(Duration::ZERO, socket, 0, connect_token, Default::default()).unwrap()
    }

    pub fn client_id(&self) -> u64 {
        self.netcode_client.client_id()
    }

    pub fn is_connected(&self) -> bool {
        self.netcode_client.connected()
    }

    /// If the client is disconnected, returns the reason.
    pub fn disconnected(&self) -> Option<DisconnectionReason> {
        if let Some(reason) = self.reliable_connection.disconnected() {
            return Some(reason.into());
        }

        if let Some(reason) = self.netcode_client.disconnected() {
            return Some(reason.into());
        }

        None
    }

    /// Disconnect the client from the server.
    pub fn disconnect(&mut self) {
        match self.netcode_client.disconnect() {
            Ok((addr, payload)) => {
                for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                    if let Err(e) = send_to(self.current_time, &self.socket, &mut self.client_packet_info, payload, addr) {
                        log::error!("failed to send disconnect packet to server: {}", e);
                    }
                }
            }
            Err(e) => log::error!("failed to generate disconnect packet: {}", e),
        }
    }

    /// Receive a message from the server over a channel.
    pub fn receive_message(&mut self, channel_id: u8) -> Option<Vec<u8>> {
        self.reliable_connection.receive_message(channel_id)
    }

    /// Send a message to the server over a channel.
    pub fn send_message(&mut self, channel_id: u8, message: Vec<u8>) {
        let message = Bytes::from(message);
        self.reliable_connection.send_message(channel_id, message);
    }

    /// Verifies if a message can be sent to the server over a channel.
    pub fn can_send_message(&self, channel_id: u8) -> bool {
        self.reliable_connection.can_send_message(channel_id)
    }

    pub fn network_info(&self) -> NetworkInfo {
        NetworkInfo {
            sent_kbps: self.client_packet_info.sent_kbps,
            received_kbps: self.client_packet_info.received_kbps,
            rtt: self.reliable_connection.rtt(),
            packet_loss: self.reliable_connection.packet_loss(),
        }
    }

    /// Send packets to the server.
    pub fn send_packets(&mut self) -> Result<(), RenetError> {
        if self.netcode_client.connected() {
            let packets = self.reliable_connection.get_packets_to_send()?;
            for packet in packets.into_iter() {
                let (addr, payload) = self.netcode_client.generate_payload_packet(&packet)?;
                send_to(self.current_time, &self.socket, &mut self.client_packet_info, payload, addr)?;
            }
        }
        Ok(())
    }

    /// Advances the client by duration, and receive packets from the network.
    pub fn update(&mut self, duration: Duration) -> Result<(), RenetError> {
        self.current_time += duration;
        self.reliable_connection.advance_time(duration);
        if let Some(reason) = self.netcode_client.disconnected() {
            return Err(NetcodeError::Disconnected(reason).into());
        }

        if let Some(reason) = self.reliable_connection.disconnected() {
            self.disconnect();
            return Err(RechannelError::ClientDisconnected(reason).into());
        }

        loop {
            let packet = match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if addr != self.netcode_client.server_addr() {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }

                    &mut self.buffer[..len]
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(RenetError::IO(e)),
            };

            let packet_info = PacketInfo::new(self.current_time, packet.len());
            self.client_packet_info.add_packet_received(packet_info);

            if let Some(payload) = self.netcode_client.process_packet(packet) {
                self.reliable_connection.process_packet(payload)?;
            }
        }

        self.reliable_connection.update()?;
        if let Some((packet, addr)) = self.netcode_client.update(duration) {
            send_to(self.current_time, &self.socket, &mut self.client_packet_info, packet, addr)?;
        }

        self.client_packet_info.update_metrics();

        Ok(())
    }
}

fn send_to(
    current_time: Duration,
    socket: &UdpSocket,
    client_packet_info: &mut ClientPacketInfo,
    packet: &[u8],
    address: SocketAddr,
) -> Result<usize, std::io::Error> {
    let packet_info = PacketInfo::new(current_time, packet.len());
    client_packet_info.add_packet_sent(packet_info);
    socket.send_to(packet, address)
}
