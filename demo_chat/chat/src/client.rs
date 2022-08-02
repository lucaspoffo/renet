use bincode::Options;
use eframe::{
    egui::{self, lerp, Color32, Layout, Pos2, Ui, Vec2},
    epaint::PathShape,
};
use egui_extras::{Size, TableBuilder};
use log::error;
use matcher::{LobbyListing, Username};
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
use crate::{ClientMessages, ServerMessages};

enum AppState {
    MainScreen {
        lobby_list: Vec<LobbyListing>,
        lobby_update: Receiver<Vec<LobbyListing>>,
    },
    RequestingToken {
        token: Receiver<ConnectToken>,
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
    username: String,
    server_addr: String,
    connection_error: Option<String>,
    text_input: String,
    last_updated: Instant,
    show_network_info: bool,
}

impl AppState {
    fn main_screen() -> Self {
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
                    log::error!("Stopped updating lobby list: {}", e);
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

fn connect_token_request(server_id: u64, username: String, sender: Sender<ConnectToken>) -> Result<(), Box<dyn Error>> {
    let client = reqwest::blocking::Client::new();
    let res = client
        .post(format!("http://localhost:7000/server/{server_id}/connect"))
        .json(&username)
        .send()?;
    res.error_for_status_ref()?;
    let bytes = res.bytes()?;
    let token = ConnectToken::read(&mut bytes.as_ref())?;
    println!("{:?}", token);
    sender.send(token)?;

    Ok(())
}

impl Default for ChatApp {
    fn default() -> Self {
        Self {
            state: AppState::main_screen(),
            username: String::new(),
            server_addr: "127.0.0.1:5000".to_string(),
            connection_error: None,
            text_input: String::new(),
            last_updated: Instant::now(),
            show_network_info: false,
        }
    }
}

impl ChatApp {
    pub fn draw(&mut self, ctx: &egui::Context) {
        match &mut self.state {
            AppState::MainScreen { .. } => self.draw_main_screen(ctx),
            AppState::RequestingToken { .. } => self.draw_connecting(ctx),
            AppState::ClientChat { client, .. } if !client.is_connected() => self.draw_connecting(ctx),
            AppState::ClientChat { .. } | AppState::HostChat { .. } => self.draw_chat(ctx),
        }
    }

    fn draw_main_screen(&mut self, ctx: &egui::Context) {
        let Self {
            username,
            server_addr,
            state,
            connection_error,
            ..
        } = self;

        let lobby_list = match state {
            AppState::MainScreen { lobby_list, .. } => lobby_list.clone(),
            _ => unreachable!(),
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Area::new("buttons")
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.set_width(300.);
                    ui.set_height(300.);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Nick:");
                            ui.text_edit_singleline(username)
                        });

                        ui.horizontal(|ui| {
                            ui.label("Server Addr:");
                            ui.text_edit_singleline(server_addr);
                        });

                        ui.vertical_centered_justified(|ui| {
                            if ui.button("Connect").clicked() {
                                match server_addr.parse::<SocketAddr>() {
                                    Err(_) => *connection_error = Some("Failed to parse server address".to_string()),
                                    Ok(server_addr) => {
                                        let client = create_renet_client(username.clone(), server_addr);

                                        *state = AppState::ClientChat {
                                            visualizer: Box::new(RenetClientVisualizer::default()),
                                            client: Box::new(client),
                                            messages: vec![],
                                            usernames: HashMap::new(),
                                        };
                                    }
                                }
                            }

                            if ui.button("Host").clicked() {
                                match server_addr.parse::<SocketAddr>() {
                                    Err(_) => *connection_error = Some("Failed to parse server address".to_string()),
                                    Ok(server_addr) => {
                                        let server = ChatServer::new(server_addr, username.clone());
                                        *state = AppState::HostChat {
                                            chat_server: Box::new(server),
                                        };
                                    }
                                }
                            }
                        });

                        if let Some(error) = connection_error {
                            ui.separator();
                            ui.colored_label(Color32::RED, format!("Connection Error: {}", error));
                        }

                        ui.separator();
                        ui.heading("Lobby list");
                        if lobby_list.is_empty() {
                            ui.label("No lobbies available");
                        } else {
                            TableBuilder::new(ui)
                                .striped(true)
                                .cell_layout(egui::Layout::left_to_right())
                                .column(Size::remainder())
                                .column(Size::exact(40.))
                                .column(Size::exact(60.))
                                .header(12.0, |mut header| {
                                    header.col(|ui| {
                                        ui.label("Name");
                                    });
                                })
                                .body(|mut body| {
                                    for lobby in lobby_list.iter() {
                                        body.row(30., |mut row| {
                                            row.col(|ui| {
                                                ui.label(&lobby.name);
                                            });

                                            row.col(|ui| {
                                                ui.label(format!("{}/{}", lobby.current_clients, lobby.max_clients));
                                            });

                                            row.col(|ui| {
                                                if ui.button("connect").clicked() {
                                                    let (sender, receiver) = mpsc::channel();
                                                    let server_id = lobby.id;
                                                    // Spawn thread to update lobby list
                                                    let username_clone = username.clone();
                                                    std::thread::spawn(move || {
                                                        if let Err(e) = connect_token_request(server_id, username_clone, sender) {
                                                            log::error!("Failed to get connect token for server {}: {}", server_id, e);
                                                        }
                                                    });

                                                    *state = AppState::RequestingToken { token: receiver };
                                                }
                                            });
                                        });
                                    }
                                });
                        }
                    });
                });
        });
    }

    fn draw_connecting(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Taken from egui progress bar widget
            let n_points = 20;
            let start_angle = ui.input().time as f64 * 360f64.to_radians();
            let end_angle = start_angle + 240f64.to_radians() * ui.input().time.sin();
            let circle_radius = 40.0;
            let center = ui.max_rect().center() - Vec2::new(circle_radius / 2., circle_radius / 2.);
            let points: Vec<Pos2> = (0..n_points)
                .map(|i| {
                    let angle = lerp(start_angle..=end_angle, i as f64 / n_points as f64);
                    let (sin, cos) = angle.sin_cos();
                    center + circle_radius * Vec2::new(cos as f32, sin as f32) + Vec2::new(circle_radius, 0.0)
                })
                .collect();
            ui.painter().add(PathShape {
                points,
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: egui::Stroke::new(2.0, ui.visuals().text_color()),
            });
        });
    }

    fn draw_chat(&mut self, ctx: &egui::Context) {
        let Self {
            text_input,
            state,
            show_network_info,
            ..
        } = self;

        let usernames = match state {
            AppState::HostChat { chat_server: server } => server.usernames.clone(),
            AppState::ClientChat { usernames, .. } => usernames.clone(),
            _ => unreachable!(),
        };

        if *show_network_info {
            match state {
                AppState::ClientChat { visualizer, .. } => {
                    visualizer.show_window(ctx);
                }
                AppState::HostChat { ref mut chat_server } => {
                    chat_server.visualizer.show_window(ctx);
                }
                _ => {}
            }
        }

        let exit = egui::SidePanel::right("right_panel")
            .min_width(200.0)
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.checkbox(show_network_info, "Show Network Graphs");

                if let AppState::HostChat { ref mut chat_server } = state {
                    draw_host_commands(ui, chat_server);
                }

                ui.vertical_centered(|ui| {
                    ui.heading("Clients");
                });

                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for username in usernames.values() {
                        ui.label(username);
                    }
                });

                let exit = ui.with_layout(Layout::bottom_up(eframe::emath::Align::Center).with_cross_justify(true), |ui| {
                    ui.button("Exit").clicked()
                });

                exit.inner
            });

        if exit.inner {
            match state {
                AppState::HostChat { chat_server } => {
                    chat_server.server.disconnect_clients();
                }
                AppState::ClientChat { client, .. } => {
                    client.disconnect();
                }
                _ => unreachable!(),
            }
            *state = AppState::main_screen();
            return;
        }

        egui::TopBottomPanel::bottom("text_editor").show(ctx, |ui| {
            let send_message = ui.horizontal(|ui| {
                let response = ui.text_edit_singleline(text_input);

                // Pressing enter makes we lose focus
                let input_send = response.lost_focus() && ui.input().key_pressed(egui::Key::Enter);
                let button_send = ui.button("Send").clicked();

                let send_message = input_send || button_send;
                if send_message {
                    response.request_focus();
                }

                send_message
            });

            if send_message.inner && !text_input.is_empty() {
                match state {
                    AppState::HostChat { chat_server } => {
                        chat_server.receive_message(1, text_input.clone());
                    }
                    AppState::ClientChat { client, .. } => {
                        let message = bincode::options().serialize(&ClientMessages::Text(text_input.clone())).unwrap();
                        client.send_message(Channels::Reliable.id(), message);
                    }
                    _ => unreachable!(),
                };
                text_input.clear();
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .id_source("client_list_scroll")
                .show(ui, |ui| {
                    let messages = match state {
                        AppState::HostChat { chat_server: server } => &server.messages,
                        AppState::ClientChat { messages, .. } => messages,
                        _ => unreachable!(),
                    };
                    for message in messages.iter() {
                        let text = if let Some(username) = usernames.get(&message.client_id) {
                            format!("{}: {}", username, message.text)
                        } else if message.client_id == 0 {
                            format!("Server: {}", message.text)
                        } else {
                            format!("unknown: {}", message.text)
                        };

                        ui.label(text);
                    }
                });
        });
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
                    self.connection_error = Some(e.to_string());
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
                                self.connection_error = None;
                                *usernames = init_usernames;
                            }
                        }
                    }
                    if let Err(e) = client.send_packets() {
                        error!("Error sending packets: {}", e);
                        self.state = AppState::main_screen();
                        self.connection_error = Some(e.to_string());
                    }
                }
            }
            AppState::HostChat { chat_server: server } => {
                if let Err(e) = server.update(duration) {
                    error!("Failed updating server: {}", e);
                    self.state = AppState::main_screen();
                    self.connection_error = Some(e.to_string());
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
                Ok(token) => {
                    let client = create_renet_client_from_token(token);

                    self.state = AppState::ClientChat {
                        visualizer: Box::new(RenetClientVisualizer::default()),
                        client: Box::new(client),
                        messages: vec![],
                        usernames: HashMap::new(),
                    };
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    panic!("Lobby request token channel disconnected");
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

fn create_renet_client(username: String, server_addr: SocketAddr) -> RenetClient {
    let client_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let socket = UdpSocket::bind(client_addr).unwrap();
    let connection_config = RenetConnectionConfig {
        send_channels_config: channels_config(),
        receive_channels_config: channels_config(),
        ..Default::default()
    };

    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    // Use current time as client_id
    let client_id = current_time.as_millis() as u64;
    let authentication = ClientAuthentication::Unsecure {
        protocol_id: 0,
        user_data: Some(Username(username).to_netcode_user_data()),
        client_id,
        server_addr,
    };

    RenetClient::new(current_time, socket, connection_config, authentication).unwrap()
}

fn draw_host_commands(ui: &mut Ui, chat_server: &mut ChatServer) {
    ui.vertical_centered(|ui| {
        ui.heading("Server Commands");
    });

    ui.separator();
    ui.horizontal(|ui| {
        let server_addr = chat_server.server.addr();
        ui.label(format!("Address: {}", server_addr));
        let tooltip = "Click to copy the server address";
        if ui.button("📋").on_hover_text(tooltip).clicked() {
            ui.output().copied_text = server_addr.to_string();
        }
    });

    ui.separator();

    egui::ScrollArea::vertical().id_source("host_commands_scroll").show(ui, |ui| {
        for client_id in chat_server.server.clients_id().into_iter() {
            ui.horizontal(|ui| {
                ui.label(format!("Client {}", client_id));
                if ui.button("X").on_hover_text("Disconnect client").clicked() {
                    chat_server.server.disconnect(client_id);
                }
            });
        }
    });
}
