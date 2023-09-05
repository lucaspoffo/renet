use std::collections::HashMap;

use renet::{ClientId, RenetServer};
use steamworks::{
    networking_sockets::{InvalidHandle, ListenSocket, NetConnection},
    networking_types::{ListenSocketEvent, NetConnectionEnd, NetworkingConfigEntry, SendFlags},
    Client, ClientManager, Manager,
};

use super::MAX_MESSAGE_BATCH_SIZE;

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct SteamServerTransport<Manager = ClientManager> {
    listen_socket: ListenSocket<Manager>,
    max_clients: usize,
    connections: HashMap<ClientId, NetConnection<Manager>>,
}

impl<T: Manager + 'static> SteamServerTransport<T> {
    pub fn new(client: &Client<T>, max_clients: usize) -> Result<Self, InvalidHandle> {
        let options: Vec<NetworkingConfigEntry> = Vec::new();
        let listen_socket = client.networking_sockets().create_listen_socket_p2p(0, options)?;
        Ok(Self {
            listen_socket,
            max_clients,
            connections: HashMap::new(),
        })
    }

    pub fn max_clients(&self) -> usize {
        self.max_clients
    }

    /// Disconnects a client from the server.
    pub fn disconnect_client(&mut self, client_id: ClientId, server: &mut RenetServer, flush_last_packets: bool) {
        if let Some((_key, value)) = self.connections.remove_entry(&client_id) {
            let _ = value.close(NetConnectionEnd::AppGeneric, Some("Client was kicked"), flush_last_packets);
        }
        server.remove_connection(client_id);
    }

    /// Disconnects all active clients including the host client from the server.
    pub fn disconnect_all(&mut self, server: &mut RenetServer, flush_last_packets: bool) {
        let keys = self.connections.keys().cloned().collect::<Vec<ClientId>>();
        for client_id in keys {
            let _ = self.connections.remove_entry(&client_id).unwrap().1.close(
                NetConnectionEnd::AppGeneric,
                Some("Client was kicked"),
                flush_last_packets,
            );
            server.remove_connection(client_id);
        }
    }

    /// Update server connections, and receive packets from the network.
    pub fn update(&mut self, server: &mut RenetServer) {
        while let Some(event) = self.listen_socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connected(event) => {
                    if let Some(steam_id) = event.remote().steam_id() {
                        let client_id = ClientId::from_raw(steam_id.raw());
                        server.add_connection(client_id);
                        self.connections.insert(client_id, event.take_connection());
                    }
                }
                ListenSocketEvent::Disconnected(event) => {
                    if let Some(steam_id) = event.remote().steam_id() {
                        let client_id = ClientId::from_raw(steam_id.raw());
                        server.remove_connection(client_id);
                        self.connections.remove(&client_id);
                    }
                }
                ListenSocketEvent::Connecting(event) => {
                    if server.connected_clients() < self.max_clients {
                        // TODO: add permissions, for now everyone is allowed to connect
                        let _ = event.accept();
                    } else {
                        event.reject(NetConnectionEnd::AppGeneric, Some("Too many clients"));
                    }
                }
            }
        }

        for (client_id, connection) in self.connections.iter_mut() {
            // TODO this allocates on the side of steamworks.rs and should be avoided, PR needed
            let messages = connection.receive_messages(MAX_MESSAGE_BATCH_SIZE);
            messages.iter().for_each(|message| {
                if let Err(e) = server.process_packet_from(message.data(), *client_id) {
                    log::error!("Error while processing payload for {}: {}", client_id, e);
                };
            });
        }
    }

    /// Send packets to connected clients.
    pub fn send_packets(&mut self, server: &mut RenetServer) {
        'clients: for client_id in server.clients_id() {
            let Some(connection) = self.connections.get(&client_id) else {
                log::error!("Error while sending packet: connection not found");
                continue;
            };
            let packets = server.get_packets_to_send(client_id).unwrap();
            // TODO: while this works fine we should probaly use the send_messages function from the listen_socket
            for packet in packets {
                if let Err(e) = connection.send_message(&packet, SendFlags::UNRELIABLE) {
                    log::error!("Failed to send packet to client {client_id}: {e}");
                    continue 'clients;
                }
            }

            if let Err(e) = connection.flush_messages() {
                log::error!("Failed flush messages for {client_id}: {e}");
            }
        }
    }
}
