use std::{
    collections::HashMap,
    io,
    net::UdpSocket,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

use matcher::{RegisterServer, ServerUpdate, Username, PROTOCOL_ID};
use renet::{
    transport::{generate_random_bytes, NetcodeServerTransport, ServerAuthentication, ServerConfig},
    ConnectionConfig, DefaultChannel, RenetServer, ServerEvent,
};
use renet_visualizer::RenetServerVisualizer;

use crate::{lobby_status::update_lobby_status, ClientMessages, Message, ServerMessages};
use bincode::Options;
use log::info;

pub struct ChatServer {
    pub server: RenetServer,
    pub transport: NetcodeServerTransport,
    pub usernames: HashMap<u64, String>,
    pub messages: Vec<Message>,
    pub visualizer: RenetServerVisualizer<240>,
    server_update: Arc<RwLock<ServerUpdate>>,
}

impl ChatServer {
    pub fn new(lobby_name: String, host_username: String, password: String) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let public_addr = socket.local_addr().unwrap();
        let private_key = generate_random_bytes();
        let server_config = ServerConfig {
            max_clients: 64,
            protocol_id: PROTOCOL_ID,
            public_addr,
            authentication: ServerAuthentication::Secure { private_key },
        };

        let password = if password.is_empty() { None } else { Some(password) };

        let register_server = RegisterServer {
            name: lobby_name,
            address: public_addr,
            max_clients: server_config.max_clients as u64,
            private_key,
            password,
            current_clients: 0,
        };
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

        let server: RenetServer = RenetServer::new(ConnectionConfig::default());

        let mut usernames = HashMap::new();
        usernames.insert(1, host_username);

        let server_update = Arc::new(RwLock::new(ServerUpdate {
            current_clients: 0,
            max_clients: transport.max_clients() as u64,
        }));

        // Create thread to register/update server status to matcher service
        let server_update_clone = server_update.clone();
        std::thread::spawn(move || {
            update_lobby_status(register_server, server_update_clone);
        });

        Self {
            server,
            transport,
            usernames,
            messages: vec![],
            visualizer: RenetServerVisualizer::default(),
            server_update,
        }
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        self.server.update(duration);
        self.transport.update(duration, &mut self.server).unwrap();

        self.visualizer.update(&self.server);

        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    let user_data = self.transport.user_data(client_id).unwrap();
                    self.visualizer.add_client(client_id);
                    let username = Username::from_user_data(&user_data).0;
                    self.usernames.insert(client_id, username.clone());
                    let message = bincode::options()
                        .serialize(&ServerMessages::ClientConnected { client_id, username })
                        .unwrap();
                    self.server.broadcast_message(DefaultChannel::ReliableOrdered, message);
                    let init_message = ServerMessages::InitClient {
                        usernames: self.usernames.clone(),
                    };
                    let init_message = bincode::options().serialize(&init_message).unwrap();
                    self.server.send_message(client_id, DefaultChannel::ReliableOrdered, init_message);
                }
                ServerEvent::ClientDisconnected { client_id, reason: _ } => {
                    self.visualizer.remove_client(client_id);
                    self.usernames.remove(&client_id);
                    let message = bincode::options()
                        .serialize(&ServerMessages::ClientDisconnected { client_id })
                        .unwrap();
                    self.server.broadcast_message(DefaultChannel::ReliableOrdered, message);
                }
            }
        }

        for client_id in self.server.connections_id() {
            while let Some(message) = self.server.receive_message(client_id, DefaultChannel::ReliableOrdered) {
                if let Ok(message) = bincode::options().deserialize::<ClientMessages>(&message) {
                    info!("Received message from client {}: {:?}", client_id, message);
                    match message {
                        ClientMessages::Text(text) => self.receive_message(client_id, text),
                    }
                }
            }
        }

        self.transport.send_packets(&mut self.server).unwrap();

        let mut server_update = self.server_update.write().unwrap();
        server_update.max_clients = self.transport.max_clients() as u64;
        server_update.current_clients = self.transport.connected_clients() as u64;

        Ok(())
    }

    pub fn receive_message(&mut self, client_id: u64, text: String) {
        let message = Message::new(client_id, text);
        self.messages.push(message.clone());
        let message = bincode::options().serialize(&ServerMessages::ClientMessage(message)).unwrap();
        self.server.broadcast_message(DefaultChannel::ReliableOrdered, message);
    }
}
