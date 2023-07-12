use std::{collections::HashMap, time::Duration};

use renet::RenetServer;
use steamworks::{
    networking_sockets::{InvalidHandle, ListenSocket, NetConnection},
    networking_types::{ListenSocketEvent, NetConnectionEnd, NetworkingConfigEntry, SendFlags},
    Client, ClientManager, ServerManager,
};

use super::{Transport, MAX_MESSAGE_BATCH_SIZE};

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct SteamTransportConfig {
    max_clients: usize,
}

impl SteamTransportConfig {
    pub fn from_max_clients(max_clients: usize) -> Self {
        Self { max_clients }
    }
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct SteamServerTransport<Manager = Client> {
    /// hold the active socket of the server
    listen_socket: ListenSocket<Manager>,
    /// hold the configuration of the server
    config: SteamTransportConfig,
    /// hold the active connections of the server (key is the client id) (value is the connection)
    connections: HashMap<u64, NetConnection<Manager>>,
}

// ClientManager implementation

impl Transport<RenetServer> for SteamServerTransport<ClientManager> {
    /// Update should run after client run the callback
    fn update(&mut self, _duration: Duration, server: &mut RenetServer) {
        self.handle_events(server);
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

    fn send_packets(&mut self, server: &mut RenetServer) {
        for client_id in self.connections.keys() {
            let packets = server.get_packets_to_send(*client_id).unwrap();
            if let Some(connection) = self.connections.get(&client_id) {
                if let Err(e) = self.send_packets_to_connection(packets, connection) {
                    log::error!("Error while sending packet: {}", e);
                }
            } else {
                log::error!("Error while sending packet: connection not found");
            }
        }
    }
}

impl SteamServerTransport<ClientManager> {
    /// Create a new server
    /// it will return [`InvalidHandle`](steamworks::networking_sockets) if the server can't be created
    /// # Arguments
    /// * `client` - the steamworks client
    /// * `config` - the configuration of the server
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
    pub fn new(client: &Client<ClientManager>, config: SteamTransportConfig) -> Result<Self, InvalidHandle> {
        let options: Vec<NetworkingConfigEntry> = Vec::new();
        let listen_socket = client.networking_sockets().create_listen_socket_p2p(0, options)?;
        Ok(Self {
            listen_socket,
            config,
            connections: HashMap::new(),
        })
    }

    pub fn max_clients(&self) -> usize {
        self.config.max_clients
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
    /// while this works fine we should probaly use the send_messages function from the listen_socket
    /// TODO to evaluate
    fn send_packets_to_connection(
        &self,
        packets: Vec<Vec<u8>>,
        connection: &NetConnection<ClientManager>,
    ) -> Result<(), steamworks::SteamError> {
        for packet in packets {
            if let Err(_e) = connection.send_message(&packet, SendFlags::UNRELIABLE) {
                continue;
            }
        }
        if let Err(e) = connection.flush_messages() {
            return Err(e);
        }
        Ok(())
    }

    /// Handle the events of the listen_socket until there are no more events
    fn handle_events(&mut self, server: &mut RenetServer) {
        let mut has_pending_events: bool = true;
        while has_pending_events {
            match self.listen_socket.try_receive_event() {
                Some(event) => match event {
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
                        if server.connected_clients() < self.config.max_clients {
                            let _ = event.accept();
                        } else {
                            event.reject(NetConnectionEnd::AppGeneric, Some("Too many clients"));
                        }
                    }
                },
                None => has_pending_events = false,
            }
        }
    }
}
// ServerManager implementation

impl Transport<RenetServer> for SteamServerTransport<ServerManager> {
    fn send_packets(&mut self, _server: &mut RenetServer) {}

    fn update(&mut self, _duration: Duration, _server: &mut RenetServer) {}
}

impl SteamServerTransport<ServerManager> {}
