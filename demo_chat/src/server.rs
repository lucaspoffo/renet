use std::{
    collections::HashMap,
    io,
    net::UdpSocket,
    time::{Duration, SystemTime},
};

use renet::{
    transport::{NativeSocket, NetcodeServerTransport, ServerAuthentication, ServerConfig},
    ClientId, ConnectionConfig, DefaultChannel, RenetServer, ServerEvent,
};
use renet_visualizer::RenetServerVisualizer;

use crate::{ClientMessages, Message, ServerMessages, Username, PROTOCOL_ID};
use bincode::Options;
use log::info;

pub const SYSTEM_MESSAGE_CLIENT_ID: ClientId = ClientId::from_raw(0);
pub const HOST_CLIENT_ID: ClientId = ClientId::from_raw(1);

pub struct ChatServer {
    pub server: RenetServer,
    pub transport: NetcodeServerTransport,
    pub usernames: HashMap<ClientId, String>,
    pub messages: Vec<Message>,
    pub visualizer: RenetServerVisualizer<240>,
}

impl ChatServer {
    pub fn new(host_username: String) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let server_config = ServerConfig {
            current_time,
            max_clients: 64,
            protocol_id: PROTOCOL_ID,
            public_addresses: vec![socket.local_addr().unwrap()],
            authentication: ServerAuthentication::Unsecure,
        };

        let transport = NetcodeServerTransport::new(server_config, NativeSocket::new(socket).unwrap()).unwrap();

        let server: RenetServer = RenetServer::new(ConnectionConfig::default());

        let mut usernames = HashMap::new();
        usernames.insert(HOST_CLIENT_ID, host_username);

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

        self.transport.send_packets(&mut self.server);

        Ok(())
    }

    pub fn receive_message(&mut self, client_id: ClientId, text: String) {
        let message = Message::new(client_id, text);
        self.messages.push(message.clone());
        let message = bincode::options().serialize(&ServerMessages::ClientMessage(message)).unwrap();
        self.server.broadcast_message(DefaultChannel::ReliableOrdered, message);
    }
}
