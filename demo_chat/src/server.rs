use std::{collections::HashMap, io, time::Duration};

#[cfg(feature = "transport")]
use std::{net::UdpSocket, time::SystemTime};

use renet::{ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
use renet_visualizer::RenetServerVisualizer;

use crate::{client::PlatformState, ClientMessages, Message, ServerMessages};
#[cfg(feature = "transport")]
use crate::Username;

#[cfg(feature = "transport")]
use renet::transport::{NetcodeServerTransport, ServerAuthentication, ServerConfig};

#[cfg(feature = "steam_transport")]
use renet_steam_transport::server::{SteamServerTransport, SteamTransportConfig};

#[cfg(feature = "transport")]
use crate::PROTOCOL_ID;

use bincode::Options;
use log::info;

pub enum ServerTransport {
    #[cfg(feature = "transport")]
    Netcode(NetcodeServerTransport),
    #[cfg(feature = "steam_transport")]
    Steam(SteamServerTransport),
}

pub struct ChatServer {
    pub server: RenetServer,
    pub transport: ServerTransport,
    pub usernames: HashMap<u64, String>,
    pub messages: Vec<Message>,
    pub visualizer: RenetServerVisualizer<240>,
}

impl ServerTransport {
    pub fn new(platform: &PlatformState) -> Self {
        match platform {
            #[cfg(feature = "transport")]
            PlatformState::Native => {
                let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
                let public_addr = socket.local_addr().unwrap();
                let server_config = ServerConfig {
                    max_clients: 64,
                    protocol_id: PROTOCOL_ID,
                    public_addr,
                    authentication: ServerAuthentication::Unsecure,
                };

                let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
                let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

                ServerTransport::Netcode(transport)
            }
            #[cfg(feature = "steam_transport")]
            PlatformState::Steam { steam_client, .. } => {
                let config = SteamTransportConfig::from_max_clients(64);
                let transport = SteamServerTransport::new(&steam_client, config).unwrap();

                ServerTransport::Steam(transport)
            }
        }
    }
}

impl ChatServer {
    pub fn new(host_username: String, platform: &PlatformState) -> Self {
        let server: RenetServer = RenetServer::new(ConnectionConfig::default());

        let transport = ServerTransport::new(platform);

        let mut usernames = HashMap::new();
        usernames.insert(1, host_username);

        Self {
            server,
            transport,
            usernames,
            messages: vec![],
            visualizer: RenetServerVisualizer::default(),
        }
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        self.server.update(duration);
        match &mut self.transport {
            #[cfg(feature = "transport")]
            ServerTransport::Netcode(netcode_transport) => {
                netcode_transport.update(duration, &mut self.server).unwrap();
            }
            #[cfg(feature = "steam_transport")]
            ServerTransport::Steam(steam_transport) => {
                steam_transport.update(duration, &mut self.server);
            }
        }

        self.visualizer.update(&self.server);

        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    // let user_data = self.transport.user_data(client_id).unwrap();
                    // let username = Username::from_user_data(&user_data).0;
                    let username = "TODO".to_string();
                    self.usernames.insert(client_id, username.clone());

                    self.visualizer.add_client(client_id);
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

        for client_id in self.server.clients_id() {
            while let Some(message) = self.server.receive_message(client_id, DefaultChannel::ReliableOrdered) {
                if let Ok(message) = bincode::options().deserialize::<ClientMessages>(&message) {
                    info!("Received message from client {}: {:?}", client_id, message);
                    match message {
                        ClientMessages::Text(text) => self.receive_message(client_id, text),
                    }
                }
            }
        }

        match &mut self.transport {
            #[cfg(feature = "transport")]
            ServerTransport::Netcode(netcode_transport) => {
                netcode_transport.send_packets(&mut self.server);
            }
            #[cfg(feature = "steam_transport")]
            ServerTransport::Steam(steam_transport) => {
                steam_transport.send_packets(&mut self.server);
            }
        }

        Ok(())
    }

    pub fn receive_message(&mut self, client_id: u64, text: String) {
        let message = Message::new(client_id, text);
        self.messages.push(message.clone());
        let message = bincode::options().serialize(&ServerMessages::ClientMessage(message)).unwrap();
        self.server.broadcast_message(DefaultChannel::ReliableOrdered, message);
    }
}
