use std::{
    collections::{HashMap, HashSet},
    net::{SocketAddr, UdpSocket},
};

use renet_udp::{
    renet::{
        error::RenetError,
        remote_connection::ConnectionConfig,
        server::{SendTarget, ServerConfig, ServerEvent},
    },
    server::UdpServer,
};

use crate::{reliable_channels_config, ClientMessages, ServerMessages};
use bincode::Options;
use log::info;

pub struct ChatServer {
    pub server: UdpServer,
    clients_initializing: HashSet<SocketAddr>,
    clients: HashMap<SocketAddr, String>,
}

impl ChatServer {
    pub fn new(addr: SocketAddr) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        let server_config = ServerConfig::default();
        let connection_config = ConnectionConfig::default();
        let server = UdpServer::new(
            server_config,
            connection_config,
            reliable_channels_config(),
            socket,
        )
        .unwrap();

        Self {
            server,
            clients_initializing: HashSet::new(),
            clients: HashMap::new(),
        }
    }

    pub fn update(&mut self) -> Result<(), RenetError> {
        self.server.update();

        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected(id) => {
                    self.clients_initializing.insert(id);
                }
                ServerEvent::ClientDisconnected(id, reason) => {
                    self.clients_initializing.remove(&id);
                    self.clients.remove(&id);
                    let message = bincode::options()
                        .serialize(&ServerMessages::ClientDisconnected(id, reason))
                        .unwrap();
                    self.server
                        .send_reliable_message(SendTarget::All, 0, message);
                }
            }
        }

        for client_id in self.server.get_clients_id().iter() {
            while let Ok(Some(message)) = self.server.receive_reliable_message(client_id, 0) {
                if let Ok(message) = bincode::options().deserialize::<ClientMessages>(&message) {
                    info!("Received message from client {}: {:?}", client_id, message);
                    match message {
                        ClientMessages::Init { nick } => {
                            if self.clients_initializing.remove(&client_id) {
                                self.clients.insert(*client_id, nick.clone());
                                let message = bincode::options()
                                    .serialize(&ServerMessages::ClientConnected(*client_id, nick))
                                    .unwrap();
                                self.server
                                    .send_reliable_message(SendTarget::All, 0, message);

                                let init_message = ServerMessages::InitClient {
                                    clients: self.clients.clone(),
                                };
                                let init_message =
                                    bincode::options().serialize(&init_message).unwrap();
                                self.server.send_reliable_message(
                                    SendTarget::Client(*client_id),
                                    0,
                                    init_message,
                                )
                            } else {
                                println!("Client not initializing");
                            }
                        }
                        ClientMessages::Text(text) => {
                            if self.clients.contains_key(client_id) {
                                let message = bincode::options()
                                    .serialize(&ServerMessages::ClientMessage(*client_id, text))
                                    .unwrap();
                                self.server
                                    .send_reliable_message(SendTarget::All, 0, message);
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
