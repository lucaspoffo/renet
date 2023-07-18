use bincode::Options;
use eframe::egui;
use log::error;
use renet::{transport::NetcodeClientTransport, DefaultChannel, RenetClient};
use renet_visualizer::RenetClientVisualizer;

use std::{collections::HashMap, time::Instant};

use crate::{
    server::ChatServer,
    ui::{draw_chat, draw_loader, draw_main_screen},
    Message, ServerMessages,
};

#[derive(Debug, Default)]
pub struct UiState {
    pub username: String,
    pub server_addr: String,
    pub error: Option<String>,
    pub text_input: String,
    pub show_network_info: bool,
}

pub enum AppState {
    MainScreen,
    ClientChat {
        client: Box<RenetClient>,
        transport: Box<NetcodeClientTransport>,
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

impl Default for ChatApp {
    fn default() -> Self {
        Self {
            state: AppState::MainScreen,
            ui_state: UiState::default(),
            last_updated: Instant::now(),
        }
    }
}

impl ChatApp {
    pub fn draw(&mut self, ctx: &egui::Context) {
        match &mut self.state {
            AppState::MainScreen => {
                draw_main_screen(&mut self.ui_state, &mut self.state, ctx);
            }
            AppState::ClientChat { transport, .. } if transport.is_connecting() => draw_loader(ctx),
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
                transport,
                usernames,
                messages,
                visualizer,
            } => {
                client.update(duration);
                if let Err(e) = transport.update(duration, client) {
                    self.state = AppState::MainScreen;
                    self.ui_state.error = Some(e.to_string());
                    return;
                }

                if let Some(e) = client.disconnect_reason() {
                    self.state = AppState::MainScreen;
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

                    if let Err(e) = transport.send_packets(client) {
                        error!("Error sending packets: {}", e);
                        self.state = AppState::MainScreen;
                        self.ui_state.error = Some(e.to_string());
                    }
                }
            }
            AppState::HostChat { chat_server: server } => {
                if let Err(e) = server.update(duration) {
                    error!("Failed updating server: {}", e);
                    self.state = AppState::MainScreen;
                    self.ui_state.error = Some(e.to_string());
                }
            }
            AppState::MainScreen => {}
        }
    }
}
