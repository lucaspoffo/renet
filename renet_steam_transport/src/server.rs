use std::{collections::HashMap, time::Duration};

use renet::RenetServer;
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
    connections: HashMap<u64, NetConnection<Manager>>,
}

impl<T: Manager + 'static> SteamServerTransport<T> {
    /// Create a new server
    /// it will return [`InvalidHandle`](steamworks::networking_sockets) if the server can't be created
    ///
    /// # Example
    ///
    /// ```
    /// use steamworks::{Client, ClientManager, networking_utils::RelayNetworkStatus};
    /// use renet_steam_transport::transport::server::{Server, SteamTransportConfig};
    ///
    /// let client = Client::init().unwrap();
    /// client.networking_utils().relay_network_status_callback(relay_network_status);
    /// client.networking_utils().init_relay_network_access();

    ///
    /// fn relay_network_status(relay: RelayNetworkStatus) {
    ///     match relay.availability() {
    ///        RelayNetworkAvailability::Current => {
    ///           let config = SteamTransportConfig { max_clients: 10 };
    ///           let mut server = Server::<ClientManager>::new(&client, config);
    ///          // do something with the server    
    ///         }
    ///         _ => {}
    /// }
    /// ```
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
    pub fn disconnect_client(&mut self, client_id: u64, server: &mut RenetServer, flush_last_packets: bool) {
        if let Some((_key, value)) = self.connections.remove_entry(&client_id) {
            let _ = value.close(NetConnectionEnd::AppGeneric, Some("Client was kicked"), flush_last_packets);
        }
        server.remove_connection(client_id);
    }

    /// Disconnects all active clients including the host client from the server.
    pub fn disconnect_all(&mut self, server: &mut RenetServer, flush_last_packets: bool) {
        let keys = self.connections.keys().cloned().collect::<Vec<u64>>();
        for client_id in keys {
            let _ = self.connections.remove_entry(&client_id).unwrap().1.close(
                NetConnectionEnd::AppGeneric,
                Some("Client was kicked"),
                flush_last_packets,
            );
            server.remove_connection(client_id);
        }
    }

    /// Update should run after client run the callback
    pub fn update(&mut self, _duration: Duration, server: &mut RenetServer) {
        while let Some(event) = self.listen_socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connected(event) => match event.remote().steam_id() {
                    Some(steam_id) => {
                        let client_id = steam_id.raw();
                        server.add_connection(client_id);
                        self.connections.insert(client_id, event.take_connection());
                    }
                    _ => {}
                },
                ListenSocketEvent::Disconnected(event) => match event.remote().steam_id() {
                    Some(steam_id) => {
                        let client_id = steam_id.raw();
                        server.remove_connection(client_id);
                        self.connections.remove(&client_id);
                    }
                    None => {}
                },
                ListenSocketEvent::Connecting(event) => {
                    if server.connected_clients() < self.max_clients {
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
                match server.process_packet_from(message.data(), *client_id) {
                    Err(e) => log::error!("Error while processing payload for {}: {}", client_id, e),
                    _ => (),
                };
            });
        }
    }

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
