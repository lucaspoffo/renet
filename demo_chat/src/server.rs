use std::{
    collections::HashMap,
    io,
    net::{SocketAddr, UdpSocket},
    time::{Duration, SystemTime},
};

use renet::{RenetConnectionConfig, RenetServer, ServerConfig, ServerEvent, NETCODE_KEY_BYTES};
use renet_visualizer::{RenetServerVisualizer, RenetVisualizerStyle};

use crate::{channels_config, Channels, ClientMessages, Message, ServerMessages, Username};
use bincode::Options;
use log::info;

pub struct ChatServer {
    pub server: RenetServer,
    pub usernames: HashMap<u64, String>,
    pub messages: Vec<Message>,
    pub visualizer: RenetServerVisualizer<240>,
}

impl ChatServer {
    pub fn new(addr: SocketAddr, private_key: &[u8; NETCODE_KEY_BYTES], host_username: String) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        let connection_config = RenetConnectionConfig {
            channels_config: channels_config(),
            ..Default::default()
        };
        let server_config = ServerConfig::new(64, 0, addr, *private_key);
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let server = RenetServer::new(current_time, server_config, connection_config, socket).unwrap();
        let mut usernames = HashMap::new();
        usernames.insert(1, host_username);

        Self {
            server,
            usernames,
            messages: vec![],
            visualizer: RenetServerVisualizer::new(RenetVisualizerStyle::default()),
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
                    self.server.broadcast_message(Channels::Reliable.id(), message);
                    let init_message = ServerMessages::InitClient {
                        usernames: self.usernames.clone(),
                    };
                    let init_message = bincode::options().serialize(&init_message).unwrap();
                    self.server.send_message(client_id, Channels::Reliable.id(), init_message);
                }
                ServerEvent::ClientDisconnected(client_id) => {
                    self.visualizer.remove_client(client_id);
                    self.usernames.remove(&client_id);
                    let message = bincode::options()
                        .serialize(&ServerMessages::ClientDisconnected { client_id })
                        .unwrap();
                    self.server.broadcast_message(Channels::Reliable.id(), message);
                }
            }
        }

        for client_id in self.server.clients_id().into_iter() {
            while let Some(message) = self.server.receive_message(client_id, Channels::Reliable.id()) {
                if let Ok(message) = bincode::options().deserialize::<ClientMessages>(&message) {
                    info!("Received message from client {}: {:?}", client_id, message);
                    match message {
                        ClientMessages::Text(text) => self.receive_message(client_id, text),
                    }
                }
            }
        }

        self.server.send_packets().unwrap();
        Ok(())
    }

    pub fn receive_message(&mut self, client_id: u64, text: String) {
        let message = Message::new(client_id, text);
        self.messages.push(message.clone());
        let message = bincode::options().serialize(&ServerMessages::ClientMessage(message)).unwrap();
        self.server.broadcast_message(Channels::Reliable.id(), message);
    }
}
