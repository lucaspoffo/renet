use renet::{
    channel::reliable::ReliableChannelConfig,
    disconnect_packet,
    error::{DisconnectionReason, RenetError},
    remote_connection::ConnectionConfig,
    server::Server,
};

use log::error;
use std::{
    collections::VecDeque,
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

#[derive(Debug)]
pub struct UdpServer {
    socket: UdpSocket,
    server: Server<SocketAddr>,
    buffer: Box<[u8]>,
    events: VecDeque<ServerEvent>,
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    ClientConnected(SocketAddr),
    ClientDisconnected(SocketAddr, DisconnectionReason),
}

impl UdpServer {
    pub fn new(
        max_clients: usize,
        connection_config: ConnectionConfig,
        reliable_channels_config: Vec<ReliableChannelConfig>,
        socket: UdpSocket,
    ) -> Result<Self, std::io::Error> {
        let buffer = vec![0u8; connection_config.max_packet_size as usize].into_boxed_slice();
        let server = Server::new(max_clients, connection_config, reliable_channels_config);
        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            server,
            buffer,
            events: VecDeque::new(),
        })
    }

    pub fn addr(&self) -> Result<SocketAddr, io::Error> {
        self.socket.local_addr()
    }

    pub fn get_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    pub fn disconnect(&mut self, client_id: &SocketAddr) {
        self.server.disconnect(client_id);
    }

    pub fn disconnect_clients(&mut self) {
        self.server.disconnect_all();
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if !self.server.is_client_connected(&addr) {
                        if let Err(reason) = self.server.add_connection(&addr) {
                            self.send_disconnect_packet(&addr, reason);
                        } else {
                            self.events.push_back(ServerEvent::ClientConnected(addr));
                        }
                    }
                    if let Err(e) = self.server.process_payload_from(&self.buffer[..len], &addr) {
                        error!("Error while processing payload for {}: {}", addr, e)
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            };
        }

        self.server.update_connections(duration);
        while let Some((client_id, reason)) = self.server.disconnected_client() {
            self.events.push_back(ServerEvent::ClientDisconnected(client_id, reason));
            self.send_disconnect_packet(&client_id, reason);
        }
        Ok(())
    }

    fn send_disconnect_packet(&self, addr: &SocketAddr, reason: DisconnectionReason) {
        if matches!(reason, DisconnectionReason::DisconnectedByClient) {
            return;
        }
        match disconnect_packet(reason) {
            Ok(packet) => {
                const NUM_DISCONNECT_PACKETS_TO_SEND: u32 = 5;
                for _ in 0..NUM_DISCONNECT_PACKETS_TO_SEND {
                    if let Err(e) = self.socket.send_to(&packet, addr) {
                        error!("failed to send disconnect packet to {}: {}", addr, e);
                    }
                }
            }
            Err(e) => {
                error!("failed to serialize disconnect packet: {}", e);
            }
        }
    }

    pub fn receive_reliable_message(&mut self, client: &SocketAddr, channel_id: u8) -> Option<Vec<u8>> {
        self.server.receive_reliable_message(client, channel_id)
    }

    pub fn receive_unreliable_message(&mut self, client: &SocketAddr) -> Option<Vec<u8>> {
        self.server.receive_unreliable_message(client)
    }

    pub fn receive_block_message(&mut self, client: &SocketAddr) -> Option<Vec<u8>> {
        self.server.receive_unreliable_message(client)
    }

    pub fn send_reliable_message(&mut self, client_id: &SocketAddr, channel_id: u8, message: Vec<u8>) -> Result<(), RenetError> {
        self.server.send_reliable_message(client_id, channel_id, message)
    }

    pub fn send_unreliable_message(&mut self, client_id: &SocketAddr, message: Vec<u8>) -> Result<(), RenetError> {
        self.server.send_unreliable_message(client_id, message)
    }

    pub fn send_block_message(&mut self, client_id: &SocketAddr, message: Vec<u8>) -> Result<(), RenetError> {
        self.server.send_block_message(client_id, message)
    }

    pub fn broadcast_reliable_message_except(&mut self, client_id: &SocketAddr, channel_id: u8, message: Vec<u8>) {
        self.server.broadcast_reliable_message_except(client_id, channel_id, message)
    }

    pub fn broadcast_unreliable_message_except(&mut self, client_id: &SocketAddr, message: Vec<u8>) {
        self.server.broadcast_unreliable_message_except(client_id, message);
    }

    pub fn broadcast_block_message_except(&mut self, client_id: &SocketAddr, message: Vec<u8>) {
        self.server.broadcast_block_message_except(client_id, message)
    }

    pub fn broadcast_reliable_message(&mut self, channel_id: u8, message: Vec<u8>) {
        self.server.broadcast_reliable_message(channel_id, message);
    }

    pub fn broadcast_unreliable_message(&mut self, message: Vec<u8>) {
        self.server.broadcast_unreliable_message(message);
    }

    pub fn broadcast_block_message(&mut self, message: Vec<u8>) {
        self.server.broadcast_block_message(message);
    }

    pub fn send_packets(&mut self) -> Result<(), io::Error> {
        for client_id in self.server.connections_id().iter() {
            let packets = match self.server.get_packets_to_send(client_id) {
                Ok(p) => p,
                Err(e) => {
                    self.server.disconnect(client_id);
                    error!("Failed to get packets from {}: {}", client_id, e);
                    continue;
                }
            };

            for packet in packets.iter() {
                self.socket.send_to(packet, client_id)?;
            }
        }
        Ok(())
    }

    pub fn clients_id(&self) -> Vec<SocketAddr> {
        self.server.connections_id()
    }
}
