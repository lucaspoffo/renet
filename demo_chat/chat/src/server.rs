use std::{
    collections::HashMap,
    io,
    net::UdpSocket,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

use matcher::{RegisterServer, ServerUpdate, Username, PROTOCOL_ID};
use renet::{generate_random_bytes, DefaultChannel, RenetConnectionConfig, RenetServer, ServerAuthentication, ServerConfig, ServerEvent};
use renet_visualizer::RenetServerVisualizer;

use crate::{lobby_status::update_lobby_status, ClientMessages, Message, ServerMessages};
use bincode::Options;
use log::info;
use steamworks::{Client, SingleClient, ClientManager};
use renet_transport_steam::{address_from_steam_id, SteamTransport};

pub struct ChatServer {
    pub server: RenetServer,
    pub usernames: HashMap<u64, String>,
    pub messages: Vec<Message>,
    pub visualizer: RenetServerVisualizer<240>,
    server_update: Arc<RwLock<ServerUpdate>>,
}

impl ChatServer {
    pub fn new(client: &Client<ClientManager>, lobby_name: String, host_username: String, password: String) -> Self {
        // let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let transport = SteamTransport::new(client);

        let server_addr = address_from_steam_id(client.user().steam_id());
        let connection_config = RenetConnectionConfig::default();
        let private_key = generate_random_bytes();
        let server_config = ServerConfig::new(64, PROTOCOL_ID, server_addr, ServerAuthentication::Secure { private_key });

        let password = if password.is_empty() { None } else { Some(password) };

        let register_server = RegisterServer {
            name: lobby_name,
            address: server_addr,
            max_clients: server_config.max_clients as u64,
            private_key,
            password,
            current_clients: 0,
        };
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let server = RenetServer::new(current_time, server_config, connection_config, Box::new(transport));
        let mut usernames = HashMap::new();
        usernames.insert(1, host_username);

        let server_update = Arc::new(RwLock::new(ServerUpdate {
            current_clients: 0,
            max_clients: server.max_clients() as u64,
        }));

        // Create thread to register/update server status to matcher service
        let server_update_clone = server_update.clone();
        std::thread::spawn(move || {
            update_lobby_status(register_server, server_update_clone);
        });

        Self {
            server,
            usernames,
            messages: vec![],
            visualizer: RenetServerVisualizer::default(),
            server_update,
        }
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        self.server.update(duration).unwrap();
        self.visualizer.update(&self.server);

        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected(client_id, user_data) => {
                    self.visualizer.add_client(client_id);
                    let username = Username::from_user_data(&user_data).0;
                    self.usernames.insert(client_id, username.clone());
                    let message = bincode::options()
                        .serialize(&ServerMessages::ClientConnected { client_id, username })
                        .unwrap();
                    self.server.broadcast_message(DefaultChannel::Reliable, message);
                    let init_message = ServerMessages::InitClient {
                        usernames: self.usernames.clone(),
                    };
                    let init_message = bincode::options().serialize(&init_message).unwrap();
                    self.server.send_message(client_id, DefaultChannel::Reliable, init_message);
                }
                ServerEvent::ClientDisconnected(client_id) => {
                    self.visualizer.remove_client(client_id);
                    self.usernames.remove(&client_id);
                    let message = bincode::options()
                        .serialize(&ServerMessages::ClientDisconnected { client_id })
                        .unwrap();
                    self.server.broadcast_message(DefaultChannel::Reliable, message);
                }
            }
        }

        for client_id in self.server.clients_id().into_iter() {
            while let Some(message) = self.server.receive_message(client_id, DefaultChannel::Reliable) {
                if let Ok(message) = bincode::options().deserialize::<ClientMessages>(&message) {
                    info!("Received message from client {}: {:?}", client_id, message);
                    match message {
                        ClientMessages::Text(text) => self.receive_message(client_id, text),
                    }
                }
            }
        }

        self.server.send_packets().unwrap();

        let mut server_update = self.server_update.write().unwrap();
        server_update.max_clients = self.server.max_clients() as u64;
        server_update.current_clients = self.server.connected_clients() as u64;

        Ok(())
    }

    pub fn receive_message(&mut self, client_id: u64, text: String) {
        let message = Message::new(client_id, text);
        self.messages.push(message.clone());
        let message = bincode::options().serialize(&ServerMessages::ClientMessage(message)).unwrap();
        self.server.broadcast_message(DefaultChannel::Reliable, message);
    }
}
