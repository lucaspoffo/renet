use crate::{
    network_info::{ClientPacketInfo, NetworkInfo, PacketInfo},
    RenetConnectionConfig, NUM_DISCONNECT_PACKETS_TO_SEND,
};

use std::{
    collections::{HashMap, VecDeque},
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use rechannel::{serialize_disconnect_packet, error::DisconnectionReason, server::RechannelServer, Bytes};

/// A server that can establish authenticated connections with multiple clients.
/// Can send/receive encrypted messages from/to them.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct RenetServer {
    reliable_server: RechannelServer<u64>,
    bandwidth_smoothing_factor: f32,
    clients_packet_info: HashMap<u64, ClientPacketInfo>,
    buffer: Box<[u8]>,
    events: VecDeque<ServerEvent>,
}

/// Events that can occur in the server.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected(u64, Option<Box<[u8]>>),
    ClientDisconnected(u64),
}

impl RenetServer {
    pub fn new(connection_config: RenetConnectionConfig) -> Self {
        let buffer = vec![0u8; connection_config.max_packet_size as usize].into_boxed_slice();
        let bandwidth_smoothing_factor = connection_config.bandwidth_smoothing_factor;
        let reliable_server = RechannelServer::new(connection_config.to_connection_config());

        Self {
            reliable_server,
            bandwidth_smoothing_factor,
            buffer,
            clients_packet_info: HashMap::new(),
            events: VecDeque::new(),
        }
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    /// Returns the client's network info if the client exits.
    pub fn network_info(&self, client_id: u64) -> Option<NetworkInfo> {
        let client_packet_info = self.clients_packet_info.get(&client_id)?;

        let sent_kbps = client_packet_info.sent_kbps;
        let received_kbps = client_packet_info.received_kbps;
        let rtt = self.reliable_server.client_rtt(client_id);
        let packet_loss = self.reliable_server.client_packet_loss(client_id);

        Some(NetworkInfo {
            received_kbps,
            sent_kbps,
            rtt,
            packet_loss,
        })
    }

    /// Advances the server by duration, and receive packets from the network.
    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        self.reliable_server.update_connections(duration);
        // TODO: Handle disconnected clients from Rechannel

        for packet_info in self.clients_packet_info.values_mut() {
            packet_info.update_metrics();
        }

        Ok(())
    }

    /// Receive a message from a client over a channel.
    pub fn receive_message<I: Into<u8>>(&mut self, client_id: u64, channel_id: I) -> Option<Vec<u8>> {
        self.reliable_server.receive_message(&client_id, channel_id)
    }

    /// Verifies if a message can be sent to a client over a channel.
    pub fn can_send_message<I: Into<u8>>(&self, client_id: u64, channel_id: I) -> bool {
        self.reliable_server.can_send_message(&client_id, channel_id)
    }

    /// Send a message to a client over a channel.
    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, client_id: u64, channel_id: I, message: B) {
        self.reliable_server.send_message(&client_id, channel_id, message);
    }

    /// Send a message to all client, except the specified one, over a channel.
    pub fn broadcast_message_except<I: Into<u8>, B: Into<Bytes>>(&mut self, client_id: u64, channel_id: I, message: B) {
        self.reliable_server.broadcast_message_except(&client_id, channel_id, message)
    }

    /// Send a message to all client over a channel.
    pub fn broadcast_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        self.reliable_server.broadcast_message(channel_id, message);
    }
}
