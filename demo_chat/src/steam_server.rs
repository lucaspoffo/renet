use std::{
    collections::HashMap,
    io,
    time::{Duration, SystemTime},
};

use renet::{ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
use renet_steam_transport::transport::{
    server::{SteamServerTransport, SteamTransportConfig},
    Transport,
};
use renet_visualizer::RenetServerVisualizer;
use steamworks::{Client, ClientManager, SingleClient};

use crate::{ClientMessages, Message, ServerMessages, Username};
use bincode::Options;
use log::info;

pub struct SteamChatServer {
    pub server: RenetServer,
    pub client: Client<ClientManager>,
    pub single_client: SingleClient,
    pub transport: SteamServerTransport<ClientManager>,
    pub usernames: HashMap<u64, String>,
    pub messages: Vec<Message>,
    pub visualizer: RenetServerVisualizer<240>,
}

impl SteamChatServer {
    pub fn new(host_username: String) -> Self {
        let steam_config = SteamTransportConfig::from_max_clients(10);

        let this = Client::init();
        let (client, single) = match this {
            Ok(t) => t,
            Err(e) => panic!("called `Result::unwrap()` on an `Err` value {}", e),
        };
        client.networking_utils().init_relay_network_access();
        // wait for relay network to be available
        for _ in 0..5 {
            single.run_callbacks();
            std::thread::sleep(::std::time::Duration::from_millis(50));
        }
        let transport = SteamServerTransport::<ClientManager>::new(&client, steam_config).unwrap();

        let server: RenetServer = RenetServer::new(ConnectionConfig::default());

        let mut usernames = HashMap::new();
        usernames.insert(1, host_username);

        Self {
            server,
            client,
            single_client: single,
            transport,
            usernames,
            messages: vec![],
            visualizer: RenetServerVisualizer::default(),
        }
    }

    pub fn update(&mut self, duration: Duration) -> Result<(), io::Error> {
        self.server.update(duration);
        self.transport.update(duration, &mut self.server);

        self.visualizer.update(&self.server);

        while let Some(event) = self.server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    //let user_data = self.transport.user_data(client_id).unwrap();
                    self.visualizer.add_client(client_id);
                    let username = Username(format!("User {}", client_id)).0;
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

    pub fn receive_message(&mut self, client_id: u64, text: String) {
        let message = Message::new(client_id, text);
        self.messages.push(message.clone());
        let message = bincode::options().serialize(&ServerMessages::ClientMessage(message)).unwrap();
        self.server.broadcast_message(DefaultChannel::ReliableOrdered, message);
    }
}
