const MAX_MESSAGE_BATCH_SIZE: usize = 255;

use std::{collections::HashMap, time::Duration};

use renet::RenetServer;
use steamworks::{
    networking_sockets::{ListenSocket, NetConnection},
    networking_types::{ListenSocketEvent, NetConnectionEnd, NetworkingConfigEntry, SendFlags},
    Client, ClientManager, ServerManager, SteamId,
};

use super::HOST_CLIENT;

pub struct SteamTransportConfig {
    max_clients: usize,
}

pub struct Server<Manager = Client> {
    /// hold the active socket of the server
    listen_socket: ListenSocket<Manager>,
    /// hold the configuration of the server
    config: SteamTransportConfig,
    /// hold the active connections of the server (key is the client id) (value is the connection)
    connections: HashMap<u64, NetConnection<Manager>>,
    /// is needed to handle the disconnect of a client (key is the steam id) (value is the client id)
    connections_steam_id: HashMap<SteamId, u64>,
    /// hold the packets for the host client
    host_queue: Vec<Vec<u8>>,
}

trait Transport {
    fn update(&mut self, duration: Duration, server: &mut RenetServer);
    fn send_packets(&mut self, server: &mut RenetServer);
}

// ClientManager implementation

impl Transport for Server<ClientManager> {
    fn update(&mut self, _duration: Duration, server: &mut RenetServer) {
        match self.listen_socket.try_receive_event() {
            Some(event) => match event {
                ListenSocketEvent::Connected(event) => {
                    let client_id = server.get_free_id();
                    match event.remote().steam_id() {
                        Some(steam_id) => {
                            self.connections_steam_id.insert(steam_id, client_id);
                        }
                        _ => {}
                    }
                    server.add_connection(client_id);
                    self.connections.insert(client_id, event.take_connection());
                }
                ListenSocketEvent::Disconnected(event) => match event.remote().steam_id() {
                    Some(steam_id) => {
                        if let Some(client_id) = self.connections_steam_id.get(&steam_id.clone()) {
                            server.remove_connection(*client_id);
                            self.connections.remove(&client_id);
                            self.connections_steam_id.remove(&steam_id);
                        }
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
            _ => {}
        }
        for (id, connection) in self.connections.iter_mut() {
            if id == &HOST_CLIENT {
                for packet in self.host_queue.iter() {
                    let _ = server.process_packet_from(&packet, *id);
                }
                self.host_queue.clear();
                continue;
            }
            // TODO this allocates on the side of steamworks.rs and should be avoided, PR needed
            let messages = connection.receive_messages(MAX_MESSAGE_BATCH_SIZE);
            messages.iter().for_each(|message| {
                let _ = server.process_packet_from(message.data(), *id);
            });
        }
    }

    fn send_packets(&mut self, server: &mut RenetServer) {
        for client_id in self.connections.keys() {
            let packets = server.get_packets_to_send(*client_id).unwrap();
            if client_id == &HOST_CLIENT {
                self.host_queue.extend(packets);
                continue;
            }
            if let Some(connection) = self.connections.get(&client_id) {
                Server::<ClientManager>::send_packet_to_connection(packets, connection, *client_id);
            }
        }
    }
}

impl Server<ClientManager> {
    /// Create a new server
    /// # Arguments
    /// * `client` - the steamworks client
    /// * `config` - the configuration of the server
    ///
    /// # Panics
    /// Panics if the [`init_relay_network_access`](steamworks::networking_utils::NetworkingUtils) was not initialized
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
    pub fn new(client: &Client<ClientManager>, config: SteamTransportConfig) -> Self {
        //  TODO this must be called at the beginning of the application
        client.networking_utils().init_relay_network_access();
        let options: Vec<NetworkingConfigEntry> = Vec::new();
        let socket;
        match client.networking_sockets().create_listen_socket_p2p(0, options) {
            Ok(listen_socket) => {
                socket = listen_socket;
            }
            Err(handle) => {
                panic!("Failed to create listen socket: {:?}", handle);
            }
        }
        Self {
            listen_socket: socket,
            config,
            connections: HashMap::new(),
            connections_steam_id: HashMap::new(),
            host_queue: Vec::new(),
        }
    }

    pub fn init_host(&mut self, server: &mut RenetServer) {
        // one time allocation only required for the host
        self.host_queue = Vec::with_capacity(MAX_MESSAGE_BATCH_SIZE);
        server.add_connection(HOST_CLIENT);
    }

    pub fn is_host_active(&self) -> bool {
        self.connections.contains_key(&HOST_CLIENT)
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
            if client_id == HOST_CLIENT {
                server.remove_connection(client_id);
                continue;
            }
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
    fn send_packet_to_connection(packets: Vec<Vec<u8>>, connection: &NetConnection<ClientManager>, client_id: u64) {
        for packet in packets {
            // TODO send reliable or unreliable depending on the packet
            if let Err(error) = connection.send_message(&packet, SendFlags::RELIABLE) {
                log::error!("Failed to send packet to client {}: {}", client_id, error);
            }
        }
        if let Err(error) = connection.flush_messages() {
            log::warn!("Failed to flush messages for client {}: {}", client_id, error);
        }
    }
}
// ServerManager implementation

impl Transport for Server<ServerManager> {
    fn send_packets(&mut self, _server: &mut RenetServer) {}

    fn update(&mut self, _duration: Duration, _server: &mut RenetServer) {}
}

impl Server<ServerManager> {}

// Extensions for the RenetServer
trait AutoGeneratedId {
    fn get_free_id(&self) -> u64;
}

impl AutoGeneratedId for RenetServer {
    fn get_free_id(&self) -> u64 {
        let id = self.clients_id().len() as u64 + 1;
        id
    }
}
