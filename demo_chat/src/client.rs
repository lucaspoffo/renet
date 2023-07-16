use bincode::Options;
use eframe::egui;
use log::error;
use renet::{DefaultChannel, RenetClient};
use renet_visualizer::RenetClientVisualizer;

#[cfg(feature = "transport")]
use renet::transport::NetcodeClientTransport;

#[cfg(feature = "steam_transport")]
use renet_steam_transport::client::SteamClientTransport;
#[cfg(feature = "steam_transport")]
use steamworks::{Client, ClientManager, SingleClient};

use std::{collections::HashMap, time::Instant};

use crate::{
    server::ChatServer,
    ui::{draw_chat, draw_loader, draw_main_screen},
    Message, ServerMessages,
};

#[derive(Debug, Default)]
pub struct UiState {
    pub username: String,
    pub transport_state: UiStateTransport,
    pub error: Option<String>,
    pub text_input: String,
    pub show_network_info: bool,
}

#[derive(Debug)]
pub enum UiStateTransport {
    #[cfg(feature = "transport")]
    Netcode { server_addr: String },
    #[cfg(feature = "steam_transport")]
    Steam { steam_server_id: u64 },
}

impl Default for UiStateTransport {
    fn default() -> Self {
        #[cfg(feature = "transport")]
        return Self::Netcode {
            server_addr: String::new(),
        };

        #[cfg(feature = "steam_transport")]
        return Self::Steam { steam_server_id: 0 };
    }
}

pub enum ClientTransport {
    #[cfg(feature = "transport")]
    Netcode(NetcodeClientTransport),
    #[cfg(feature = "steam_transport")]
    Steam(SteamClientTransport),
}

pub enum AppState {
    MainScreen,
    ClientChat {
        client: Box<RenetClient>,
        transport: ClientTransport,
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
    platform: PlatformState,
}

pub enum PlatformState {
    #[cfg(feature = "transport")]
    Native,
    #[cfg(feature = "steam_transport")]
    Steam {
        steam_client: Client<ClientManager>,
        steam_single: SingleClient<ClientManager>,
    },
}

impl Default for ChatApp {
    #[cfg(feature = "transport")]
    fn default() -> Self {
        Self {
            state: AppState::MainScreen,
            ui_state: UiState::default(),
            last_updated: Instant::now(),
            platform: PlatformState::Native,
        }
    }

    #[cfg(feature = "steam_transport")]
    fn default() -> Self {
        let (steam_client, steam_single) = Client::init().unwrap();

        Self {
            state: AppState::MainScreen,
            ui_state: UiState::default(),
            platform: PlatformState::Steam {
                steam_client,
                steam_single,
            },
            last_updated: Instant::now(),
        }
    }
}

impl ClientTransport {
    pub fn is_connecting(&self) -> bool {
        match self {
            #[cfg(feature = "transport")]
            ClientTransport::Netcode(netcode_transport) => netcode_transport.is_connecting(),
            #[cfg(feature = "steam_transport")]
            ClientTransport::Steam(steam_transport) => steam_transport.is_connecting(),
        }
    }
}

impl ChatApp {
    pub fn draw(&mut self, ctx: &egui::Context) {
        match &mut self.state {
            AppState::MainScreen => {
                draw_main_screen(&mut self.ui_state, &mut self.state, ctx, &self.platform);
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

        match &self.platform {
            #[cfg(feature = "steam_transport")]
            PlatformState::Steam { steam_single, .. } => {
                steam_single.run_callbacks();
            }
            #[cfg(feature = "transport")]
            PlatformState::Native => {}
        }

        match &mut self.state {
            AppState::ClientChat {
                client,
                transport,
                usernames,
                messages,
                visualizer,
            } => {
                client.update(duration);
                match transport {
                    #[cfg(feature = "transport")]
                    ClientTransport::Netcode(netcode_transport) => {
                        if let Err(e) = transport.update(duration, client) {
                            self.state = AppState::MainScreen;
                            self.ui_state.error = Some(e.to_string());
                            return;
                        }
                    }
                    #[cfg(feature = "steam_transport")]
                    ClientTransport::Steam(steam_transport) => {
                        steam_transport.update(duration, client);
                    }
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

                    match transport {
                        #[cfg(feature = "transport")]
                        ClientTransport::Netcode(netcode_transport) => {
                            if let Err(e) = transport.send_packets(client) {
                                error!("Error sending packets: {}", e);
                                self.state = AppState::MainScreen;
                                self.ui_state.error = Some(e.to_string());
                            }
                        }
                        #[cfg(feature = "steam_transport")]
                        ClientTransport::Steam(steam_transport) => {
                            steam_transport.send_packets(client);
                        }
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
