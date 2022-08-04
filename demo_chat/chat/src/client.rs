use bincode::Options;
use eframe::egui;
use log::error;
use matcher::{LobbyListing, RequestConnection};
use renet::{ClientAuthentication, ConnectToken, RenetClient, RenetConnectionConfig};
use renet_visualizer::RenetClientVisualizer;

use std::{
    collections::HashMap,
    error::Error,
    net::{SocketAddr, UdpSocket},
    sync::mpsc::{Receiver, TryRecvError},
    time::{Instant, SystemTime},
};
use std::{
    sync::mpsc::{self, Sender},
    time::Duration,
};

use crate::{channels_config, server::ChatServer, Channels, Message};
use crate::{
    ui::{draw_chat, draw_loader, draw_main_screen},
    ServerMessages,
};

#[derive(Debug, Default)]
pub struct UiState {
    pub username: String,
    pub password: String,
    pub lobby_name: String,
    pub error: Option<String>,
    pub text_input: String,
    pub show_network_info: bool,
}

pub enum AppState {
    MainScreen {
        lobby_list: Vec<LobbyListing>,
        lobby_update: Receiver<Vec<LobbyListing>>,
    },
    RequestingToken {
        token: Receiver<reqwest::Result<ConnectToken>>,
    },
    ClientChat {
        client: Box<RenetClient>,
        usernames: HashMap<u64, String>,
        messages: Vec<Message>,
        visualizer: Box<RenetClientVisualizer<240>>,
    },
    HostChat {
        chat_server: Box<ChatServer>,
    },
}

pub struct ChatApp {
    state: AppState,
    ui_state: UiState,
    last_updated: Instant,
}

impl AppState {
    pub fn main_screen() -> Self {
        let (sender, receiver) = mpsc::channel();

        // Spawn thread to update lobby list
        std::thread::spawn(move || {
            update_lobby(sender);
        });

        AppState::MainScreen {
            lobby_list: vec![],
            lobby_update: receiver,
        }
    }
}

fn update_lobby(sender: Sender<Vec<LobbyListing>>) {
    let client = reqwest::blocking::Client::new();
    loop {
        match lobby_list_request(&client) {
            Err(e) => log::error!("Error getting lobby list: {}", e),
            Ok(lobby_list) => {
                if let Err(e) = sender.send(lobby_list) {
                    log::info!("Stopped updating lobby list: {}", e);
                    break;
                }
            }
        }

        std::thread::sleep(Duration::from_secs(5));
    }
}

fn lobby_list_request(client: &reqwest::blocking::Client) -> Result<Vec<LobbyListing>, reqwest::Error> {
    let res = client.get("http://localhost:7000/server").send()?;
    res.error_for_status_ref()?;
    let lobby_list: Vec<LobbyListing> = res.json()?;

    Ok(lobby_list)
}

pub fn connect_token_request(
    server_id: u64,
    request_connection: RequestConnection,
    sender: Sender<reqwest::Result<ConnectToken>>,
) -> Result<(), Box<dyn Error>> {
    let client = reqwest::blocking::Client::new();
    let res = client
        .post(format!("http://localhost:7000/server/{server_id}/connect"))
        .json(&request_connection)
        .send()?;
    if let Err(e) = res.error_for_status_ref() {
        sender.send(Err(e))?;
    } else {
        let bytes = res.bytes()?;
        let token = ConnectToken::read(&mut bytes.as_ref())?;
        sender.send(Ok(token))?;
    }

    Ok(())
}

impl Default for ChatApp {
    fn default() -> Self {
        Self {
            state: AppState::main_screen(),
            ui_state: UiState::default(),
            last_updated: Instant::now(),
        }
    }
}

impl ChatApp {
    pub fn draw(&mut self, ctx: &egui::Context) {
        match &mut self.state {
            AppState::MainScreen { lobby_list, .. } => {
                let lobby_list = lobby_list.clone();
                draw_main_screen(&mut self.ui_state, &mut self.state, lobby_list, ctx);
            }
            AppState::RequestingToken { .. } => draw_loader(ctx),
            AppState::ClientChat { client, .. } if !client.is_connected() => draw_loader(ctx),
            AppState::ClientChat { usernames, .. } => {
                let usernames = usernames.clone();
                draw_chat(&mut self.ui_state, &mut self.state, usernames, ctx);
            }
            AppState::HostChat { chat_server } => {
                let usernames = chat_server.usernames.clone();
                draw_chat(&mut self.ui_state, &mut self.state, usernames, ctx);
            }
        }
    }

    pub fn update_chat(&mut self) {
        let now = Instant::now();
        let duration = now - self.last_updated;
        self.last_updated = now;

        match &mut self.state {
            AppState::ClientChat {
                client,
                usernames,
                messages,
                visualizer,
            } => {
                if let Err(e) = client.update(duration) {
                    error!("{}", e);
                }
                if let Some(e) = client.disconnected() {
                    self.state = AppState::main_screen();
                    self.ui_state.error = Some(e.to_string());
                } else {
                    visualizer.add_network_info(client.network_info());

                    while let Some(message) = client.receive_message(Channels::Reliable.id()) {
                        let message: ServerMessages = bincode::options().deserialize(&message).unwrap();
                        match message {
                            ServerMessages::ClientConnected { client_id, username } => {
                                usernames.insert(client_id, username);
                            }
                            ServerMessages::ClientDisconnected { client_id } => {
                                usernames.remove(&client_id);
                                let text = format!("client {} disconnect", client_id);
                                messages.push(Message::new(0, text));
                            }
                            ServerMessages::ClientMessage(message) => {
                                messages.push(message);
                            }
                            ServerMessages::InitClient { usernames: init_usernames } => {
                                self.ui_state.error = None;
                                *usernames = init_usernames;
                            }
                        }
                    }
                    if let Err(e) = client.send_packets() {
                        error!("Error sending packets: {}", e);
                        self.state = AppState::main_screen();
                        self.ui_state.error = Some(e.to_string());
                    }
                }
            }
            AppState::HostChat { chat_server: server } => {
                if let Err(e) = server.update(duration) {
                    error!("Failed updating server: {}", e);
                    self.state = AppState::main_screen();
                    self.ui_state.error = Some(e.to_string());
                }
            }
            AppState::MainScreen {
                lobby_list, lobby_update, ..
            } => match lobby_update.try_recv() {
                Ok(new_list) => *lobby_list = new_list,
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    panic!("Lobby update channel disconnected");
                }
            },
            AppState::RequestingToken { token } => match token.try_recv() {
                Ok(Ok(token)) => {
                    let client = create_renet_client_from_token(token);

                    self.state = AppState::ClientChat {
                        visualizer: Box::new(RenetClientVisualizer::default()),
                        client: Box::new(client),
                        messages: vec![],
                        usernames: HashMap::new(),
                    };
                }
                Ok(Err(e)) => {
                    self.ui_state.error = Some(format!("Failed to get connect token:\n{}", e));
                    self.state = AppState::main_screen();
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.ui_state.error = Some("Failed to get connect token".to_owned());
                    self.state = AppState::main_screen();
                }
            },
        }
    }
}

fn create_renet_client_from_token(connect_token: ConnectToken) -> RenetClient {
    let client_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let socket = UdpSocket::bind(client_addr).unwrap();
    let connection_config = RenetConnectionConfig {
        send_channels_config: channels_config(),
        receive_channels_config: channels_config(),
        ..Default::default()
    };

    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let authentication = ClientAuthentication::Secure { connect_token };

    RenetClient::new(current_time, socket, connection_config, authentication).unwrap()
}
