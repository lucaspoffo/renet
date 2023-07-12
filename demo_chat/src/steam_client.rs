use bincode::Options;
use eframe::egui;
use log::error;
use renet::{DefaultChannel, RenetClient};
use renet_steam_transport::transport::{client::SteamClientTransport, Transport};
use renet_visualizer::RenetClientVisualizer;
use steamworks::SingleClient;

use std::{collections::HashMap, fmt::Display, time::Instant};

use crate::{
    steam_server::SteamChatServer,
    steam_ui::{draw_chat, draw_loader, draw_main_screen},
    Message, ServerMessages,
};

#[derive(Debug, Default)]
pub struct SteamUiState {
    pub username: String,
    pub server_addr: String,
    pub error: Option<String>,
    pub text_input: String,
    pub show_network_info: bool,
}

pub enum SteamAppState {
    MainScreen,
    ClientChat {
        client: Box<RenetClient>,
        single_client: SingleClient,
        transport: SteamClientTransport,
        usernames: HashMap<u64, String>,
        messages: Vec<Message>,
        visualizer: Box<RenetClientVisualizer<240>>,
    },
    HostChat {
        chat_server: Box<SteamChatServer>,
    },
}

impl Display for SteamAppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SteamAppState::MainScreen => write!(f, "MainScreen"),
            SteamAppState::ClientChat { .. } => write!(f, "ClientChat"),
            SteamAppState::HostChat { .. } => write!(f, "HostChat"),
        }
    }
}

pub struct SteamChatApp {
    state: SteamAppState,
    ui_state: SteamUiState,
    last_updated: Instant,
}

impl Default for SteamChatApp {
    fn default() -> Self {
        Self {
            state: SteamAppState::MainScreen,
            ui_state: SteamUiState::default(),
            last_updated: Instant::now(),
        }
    }
}

impl SteamChatApp {
    pub fn draw(&mut self, ctx: &egui::Context) {
        match &mut self.state {
            SteamAppState::MainScreen => {
                draw_main_screen(&mut self.ui_state, &mut self.state, ctx);
            }
            SteamAppState::ClientChat { transport, .. } if transport.is_connecting() => draw_loader(ctx),
            SteamAppState::ClientChat { usernames, .. } => {
                let usernames = usernames.clone();
                draw_chat(&mut self.ui_state, &mut self.state, usernames, ctx);
            }
            SteamAppState::HostChat { chat_server } => {
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
            SteamAppState::ClientChat {
                client,
                transport,
                usernames,
                messages,
                single_client,
                visualizer,
            } => {
                single_client.run_callbacks();
                client.update(duration);
                if transport.is_connected() {
                    transport.update(duration, client);
                }

                if let Some(e) = client.disconnect_reason() {
                    self.state = SteamAppState::MainScreen;
                    self.ui_state.error = Some(e.to_string());
                } else {
                    visualizer.add_network_info(client.network_info());

                    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
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
                    if transport.is_connected() {
                        transport.send_packets(client)
                    }
                }
            }
            SteamAppState::HostChat { chat_server: server } => {
                if let Err(e) = server.update(duration) {
                    error!("Failed updating server: {}", e);
                    self.state = SteamAppState::MainScreen;
                    self.ui_state.error = Some(e.to_string());
                }
            }
            SteamAppState::MainScreen => {}
        }
    }
}
