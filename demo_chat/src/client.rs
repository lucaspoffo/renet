use bincode::Options;
use eframe::{
    egui::{self, lerp, Color32, Layout, Pos2, Ui, Vec2},
    epaint::PathShape,
};
use log::error;
use renet::{ConnectToken, NetworkInfo, RenetClient, RenetConnectionConfig, NETCODE_KEY_BYTES};
use renet_visualizer::{RenetClientVisualizer, RenetVisualizerStyle};

use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    time::{Instant, SystemTime},
};

use crate::{channels_config, server::ChatServer, Channels, Message, Username};
use crate::{ClientMessages, ServerMessages};

enum AppState {
    MainScreen,
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
    private_key: String,
    connection_error: Option<String>,
    text_input: String,
    last_updated: Instant,
    show_network_info: bool,
}

impl Default for AppState {
    fn default() -> Self {
        AppState::MainScreen
    }
}

impl Default for ChatApp {
    fn default() -> Self {
        Self {
            state: AppState::MainScreen,
            username: String::new(),
            server_addr: "127.0.0.1:5000".to_string(),
            private_key: "an example very very secret key.".to_string(),
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
            AppState::MainScreen => self.draw_main_screen(ctx),
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
            private_key,
            ..
        } = self;

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Area::new("buttons")
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.set_width(300.);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Nick:");
                            ui.text_edit_singleline(username)
                        });

                        ui.horizontal(|ui| {
                            ui.label("Private Key:");
                            ui.text_edit_singleline(private_key);
                            private_key.truncate(NETCODE_KEY_BYTES);
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
                                        let mut key: [u8; NETCODE_KEY_BYTES] = [0; NETCODE_KEY_BYTES];
                                        if private_key.as_bytes().len() != NETCODE_KEY_BYTES {
                                            *connection_error = Some("Private key must have 32 bytes.".to_string());
                                        } else {
                                            key.copy_from_slice(private_key.as_bytes());
                                            let client = create_renet_client(username.clone(), server_addr, &key);

                                            *state = AppState::ClientChat {
                                                visualizer: Box::new(RenetClientVisualizer::new(RenetVisualizerStyle::default())),
                                                client: Box::new(client),
                                                messages: vec![],
                                                usernames: HashMap::new(),
                                            };
                                        }
                                    }
                                }
                            }

                            if ui.button("Host").clicked() {
                                match server_addr.parse::<SocketAddr>() {
                                    Err(_) => *connection_error = Some("Failed to parse server address".to_string()),
                                    Ok(server_addr) => {
                                        let mut key: [u8; NETCODE_KEY_BYTES] = [0; NETCODE_KEY_BYTES];
                                        if private_key.as_bytes().len() != NETCODE_KEY_BYTES {
                                            *connection_error = Some("Private key must have 32 bytes.".to_string());
                                        } else {
                                            key.copy_from_slice(private_key.as_bytes());
                                            let server = ChatServer::new(server_addr, &key, username.clone());
                                            *state = AppState::HostChat {
                                                chat_server: Box::new(server),
                                            };
                                        }
                                    }
                                }
                            }
                        });

                        if let Some(error) = connection_error {
                            ui.separator();
                            ui.colored_label(Color32::RED, format!("Connection Error: {}", error));
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
            *state = AppState::MainScreen;
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
                    self.state = AppState::MainScreen;
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
                        self.state = AppState::MainScreen;
                        self.connection_error = Some(e.to_string());
                    }
                }
            }
            AppState::HostChat { chat_server: server } => {
                if let Err(e) = server.update(duration) {
                    error!("Failed updating server: {}", e);
                    self.state = AppState::MainScreen;
                    self.connection_error = Some(e.to_string());
                }
            }
            _ => {}
        }
    }
}

fn create_renet_client(username: String, server_addr: SocketAddr, private_key: &[u8; NETCODE_KEY_BYTES]) -> RenetClient {
    let client_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let socket = UdpSocket::bind(client_addr).unwrap();
    let connection_config = RenetConnectionConfig {
        channels_config: channels_config(),
        ..Default::default()
    };

    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    // Use current time as client_id
    let client_id = current_time.as_millis() as u64;
    let user_data = Username(username).to_netcode_user_data();
    let connect_token = ConnectToken::generate(
        current_time,
        0,
        300,
        client_id,
        15,
        vec![server_addr],
        Some(&user_data),
        private_key,
    )
    .unwrap();

    RenetClient::new(current_time, socket, client_id, connect_token, connection_config).unwrap()
}

fn draw_network_info(ui: &mut Ui, network_info: &NetworkInfo) {
    ui.vertical(|ui| {
        ui.heading("Network info");
        ui.label(format!("RTT: {:.2} ms", network_info.rtt));
        ui.label(format!("Sent Kbps: {:.2}", network_info.sent_kbps));
        ui.label(format!("Received Kbps: {:.2}", network_info.received_kbps));
        ui.label(format!("Packet Loss: {:.2}%", network_info.packet_loss * 100.));
    });
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
                ui.label(format!("Client {}", client_id)).on_hover_ui(|ui| {
                    let network_info = chat_server.server.network_info(client_id).unwrap();
                    draw_network_info(ui, &network_info);
                });
                if ui.button("X").on_hover_text("Disconnect client").clicked() {
                    chat_server.server.disconnect(client_id);
                }
            });
        }
    });
}
