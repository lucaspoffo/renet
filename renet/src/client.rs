use crate::{
    error::{DisconnectionReason, RenetError},
    RenetConnectionConfig, NUM_DISCONNECT_PACKETS_TO_SEND,
};

use rechannel::{
    error::RechannelError,
    remote_connection::{NetworkInfo, RemoteConnection},
};
use renetcode::{ConnectToken, NetcodeClient, NetcodeError, PacketToSend};

use log::debug;

use std::sync::mpsc::TryRecvError;
use std::time::Duration;
use std::{
    net::SocketAddr,
    sync::mpsc::{Receiver, Sender},
};

/// A client that establishes an authenticated connection with a server.
/// Can send/receive encrypted messages from/to the server.
pub struct RenetClient {
    netcode_client: NetcodeClient,
    reliable_connection: RemoteConnection,
    packet_sender: Sender<(SocketAddr, Vec<u8>)>,
    packet_receiver: Receiver<(SocketAddr, Vec<u8>)>,
}

impl RenetClient {
    pub fn new(
        current_time: Duration,
        client_id: u64,
        connect_token: ConnectToken,
        config: RenetConnectionConfig,
        packet_sender: Sender<(SocketAddr, Vec<u8>)>,
        packet_receiver: Receiver<(SocketAddr, Vec<u8>)>,
    ) -> Self {
        let reliable_connection = RemoteConnection::new(config.to_connection_config());
        let netcode_client = NetcodeClient::new(current_time, client_id, connect_token);

        Self {
            reliable_connection,
            netcode_client,
            packet_receiver,
            packet_sender,
        }
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
            Ok(PacketToSend { packet, address }) => {
                for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                    if let Err(e) = self.packet_sender.send((address, packet.to_vec())) {
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
        self.reliable_connection.send_message(channel_id, message);
    }

    /// Verifies if a message can be sent to the server over a channel.
    pub fn can_send_message(&self, channel_id: u8) -> bool {
        self.reliable_connection.can_send_message(channel_id)
    }

    pub fn network_info(&self) -> &NetworkInfo {
        self.reliable_connection.network_info()
    }

    /// Send packets to the server.
    pub fn send_packets(&mut self) -> Result<(), RenetError> {
        if self.netcode_client.connected() {
            let packets = self.reliable_connection.get_packets_to_send()?;
            for packet in packets.into_iter() {
                let PacketToSend { packet, address } = self.netcode_client.generate_payload_packet(&packet)?;
                self.packet_sender
                    .send((address, packet.to_vec()))
                    .map_err(|_| RenetError::SenderDisconnected)?;
            }
        }
        Ok(())
    }

    /// Advances the client by duration, and receive packets from the network.
    pub fn update(&mut self, duration: Duration) -> Result<(), RenetError> {
        self.reliable_connection.advance_time(duration);
        if let Some(reason) = self.netcode_client.disconnected() {
            return Err(NetcodeError::Disconnected(reason).into());
        }

        if let Some(reason) = self.reliable_connection.disconnected() {
            self.disconnect();
            return Err(RechannelError::ClientDisconnected(reason).into());
        }

        loop {
            let mut packet = match self.packet_receiver.try_recv() {
                Ok((addr, payload)) => {
                    if addr != self.netcode_client.server_addr() {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }

                    payload
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(RenetError::ReceiverDisconnected),
            };

            if let Some(payload) = self.netcode_client.process_packet(&mut packet) {
                self.reliable_connection.process_packet(payload)?;
            }
        }

        self.reliable_connection.update()?;
        if let Some((packet, addr)) = self.netcode_client.update(duration) {
            self.packet_sender
                .send((addr, packet.to_vec()))
                .map_err(|_| RenetError::SenderDisconnected)?;
        }

        Ok(())
    }
}
