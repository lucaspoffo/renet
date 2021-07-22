use std::{
    collections::{HashMap, HashSet},
    net::{SocketAddr, UdpSocket},
};

use renet::{
    protocol::unsecure::UnsecureServerProtocol,
    remote_connection::ConnectionConfig,
    server::{Server, ServerConfig, ServerEvent},
    RenetError,
};

use crate::{channels_config, ClientMessages, ServerMessages};
use log::info;

pub struct ChatServer {
    pub server: Server<UnsecureServerProtocol>,
    clients_initializing: HashSet<u64>,
    clients: HashMap<u64, String>,
}

impl ChatServer {
    pub fn new(addr: SocketAddr) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        let server_config = ServerConfig::default();
        let connection_config = ConnectionConfig::default();
        let server: Server<UnsecureServerProtocol> =
            Server::new(socket, server_config, connection_config, channels_config()).unwrap();

        Self {
            server,
            clients_initializing: HashSet::new(),
            clients: HashMap::new(),
        }
    }

    pub fn update(&mut self) -> Result<(), RenetError> {
        self.server.update()?;

        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected(id) => {
                    self.clients_initializing.insert(id);
                }
                ServerEvent::ClientDisconnected(id) => {
                    self.clients_initializing.remove(&id);
                    self.clients.remove(&id);
                }
            }
        }

        for client_id in self.server.get_clients_id().into_iter() {
            while let Ok(Some(message)) = self.server.receive_message(client_id, 0) {
                if let Ok(message) = bincode::deserialize::<ClientMessages>(&message) {
                    info!("Received message from client {}: {:?}", client_id, message);
                    match message {
                        ClientMessages::Init { nick } => {
                            if self.clients_initializing.remove(&client_id) {
                                self.clients.insert(client_id, nick.clone());
                                let message =
                                    bincode::serialize(&ServerMessages::ClientConnected(nick))
                                        .unwrap();
                                self.server.broadcast_message(0, message);

                                let init_message = ServerMessages::InitClient {
                                    clients: self.clients.values().cloned().collect(),
                                };
                                let init_message = bincode::serialize(&init_message).unwrap();
                                self.server
                                    .send_message(client_id, 0, init_message)
                                    .unwrap();
                            }
                        }
                        ClientMessages::Text(text) => {
                            if let Some(client) = self.clients.get(&client_id) {
                                let message = bincode::serialize(&ServerMessages::ClientMessage(
                                    client.clone(),
                                    text,
                                ))
                                .unwrap();
                                self.server.broadcast_message(0, message);
                            }
                        }
                    }
                }
            }
        }

        self.server.send_packets();
        Ok(())
    }
}
