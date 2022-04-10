use std::{
    collections::HashMap,
    io,
    net::{SocketAddr, UdpSocket},
    time::{Duration, SystemTime},
};

use renet::{RenetConnectionConfig, RenetServer, ServerConfig, ServerEvent, NETCODE_KEY_BYTES};

use crate::{ClientMessages, ServerMessages, Username};
use bincode::Options;
use log::info;

pub struct ChatServer {
    pub server: RenetServer,
    usernames: HashMap<u64, String>,
}

impl ChatServer {
    pub fn new(addr: SocketAddr, private_key: &[u8; NETCODE_KEY_BYTES]) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        let connection_config = RenetConnectionConfig::default();
        let server_config = ServerConfig::new(64, 0, addr, *private_key);
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let server = RenetServer::new(current_time, server_config, connection_config, socket).unwrap();

        Self { server, usernames: HashMap::new() }
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        self.server.update(duration).unwrap();

        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected(id, user_data) => {
                    let username = Username::from_user_data(&user_data);
                    self.usernames.insert(id, username.0.clone());
                    let message = bincode::options()
                        .serialize(&ServerMessages::ClientConnected(id, username.0))
                        .unwrap();
                    self.server.broadcast_message(0, message);
                    let init_message = ServerMessages::InitClient { usernames: self.usernames.clone() };
                    let init_message = bincode::options().serialize(&init_message).unwrap();
                    self.server.send_message(id, 0, init_message).unwrap();
                }
                ServerEvent::ClientDisconnected(id) => {
                    self.usernames.remove(&id);
                    let message = bincode::options().serialize(&ServerMessages::ClientDisconnected(id)).unwrap();
                    self.server.broadcast_message(0, message);
                }
            }
        }

        for client_id in self.server.clients_id().into_iter() {
            while let Some(message) = self.server.receive_message(client_id, 0) {
                if let Ok(message) = bincode::options().deserialize::<ClientMessages>(&message) {
                    info!("Received message from client {}: {:?}", client_id, message);
                    match message {
                        ClientMessages::Text(message_id, text) => {
                            if self.usernames.contains_key(&client_id) {
                                let client_message = bincode::options()
                                    .serialize(&ServerMessages::ClientMessage(client_id, text))
                                    .unwrap();
                                self.server.broadcast_message_except(client_id, 0, client_message);
                                let received_message = bincode::options().serialize(&ServerMessages::MessageReceived(message_id)).unwrap();
                                if let Err(e) = self.server.send_message(client_id, 0, received_message) {
                                    log::error!("Error sending confirmation message: {}", e);
                                    self.server.disconnect(client_id);
                                }
                            }
                        }
                    }
                }
            }
        }

        self.server.send_packets().unwrap();
        Ok(())
    }
}
