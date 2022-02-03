use crate::{RenetConnnectionConfig, RenetError};

use rechannel::{
    error::{DisconnectionReason, RechannelError},
    remote_connection::{NetworkInfo, RemoteConnection},
};
use renetcode::{ConnectToken, NetcodeClient, NetcodeError, NETCODE_MAX_PACKET_BYTES};

use log::debug;

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

pub struct RenetClient {
    id: SocketAddr,
    netcode_client: NetcodeClient,
    socket: UdpSocket,
    reliable_connection: RemoteConnection,
    buffer: [u8; NETCODE_MAX_PACKET_BYTES],
}

impl RenetClient {
    pub fn new(
        current_time: Duration,
        addr: SocketAddr,
        connect_token: ConnectToken,
        config: RenetConnnectionConfig,
    ) -> Result<Self, std::io::Error> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        let id = socket.local_addr()?;
        let reliable_connection = RemoteConnection::new(config.to_connection_config());
        let netcode_client = NetcodeClient::new(current_time, connect_token);

        Ok(Self {
            buffer: [0u8; NETCODE_MAX_PACKET_BYTES],
            socket,
            id,
            reliable_connection,
            netcode_client,
        })
    }

    pub fn id(&self) -> SocketAddr {
        self.id
    }

    pub fn is_connected(&self) -> bool {
        self.reliable_connection.is_connected()
    }

    pub fn disconnected(&self) -> Option<DisconnectionReason> {
        self.reliable_connection.disconnected()
    }

    pub fn disconnect(&mut self) {
        match self.netcode_client.disconnect() {
            Ok((packet, server_addr)) => {
                const NUM_DISCONNECT_PACKETS_TO_SEND: u32 = 5;
                for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                    if let Err(e) = self.socket.send_to(&packet, server_addr) {
                        log::error!("failed to send disconnect packet to server: {}", e);
                    }
                }
            }
            Err(e) => {
                log::error!("failed to generate disconnect packet: {}", e);
            }
        }
    }

    pub fn receive_message(&mut self, channel_id: u8) -> Option<Vec<u8>> {
        self.reliable_connection.receive_message(channel_id)
    }

    pub fn send_message(&mut self, channel_id: u8, message: Vec<u8>) -> Result<(), RechannelError> {
        self.reliable_connection.send_message(channel_id, message)
    }

    pub fn network_info(&self) -> &NetworkInfo {
        self.reliable_connection.network_info()
    }

    pub fn send_packets(&mut self) -> Result<(), RenetError> {
        let packets = self.reliable_connection.get_packets_to_send()?;
        for packet in packets.into_iter() {
            let (packet, server_addr) = self.netcode_client.generate_payload_packet(&packet)?;
            self.socket.send_to(&packet, server_addr)?;
        }
        Ok(())
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), RenetError> {
        if let Some(_) = self.netcode_client.error() {
            return Err(NetcodeError::Disconnected.into());
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
                Err(e) => return Err(RenetError::IOError(e)),
            };

            if let Some(payload) = self.netcode_client.process_packet(packet) {
                self.reliable_connection.process_packet(payload)?;
            }
        }

        self.reliable_connection.update()?;
        if let Some((packet, addr)) = self.netcode_client.update(duration) {
            self.socket.send_to(packet, addr)?;
        }

        Ok(())
    }
}
