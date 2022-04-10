use bincode::Options;
use eframe::{
    egui::{self, lerp, Color32, Pos2, Ui, Vec2},
    epaint::PathShape,
};
use log::error;
use renet::{ConnectToken, RenetClient, RenetConnectionConfig, NETCODE_KEY_BYTES};

use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    time::{Duration, SystemTime},
};

use crate::{server::ChatServer, Username};
use crate::{ClientMessages, ServerMessages};

#[derive(Debug)]
enum AppState {
    Start,
    Connecting,
    Chat,
}

impl Default for AppState {
    fn default() -> Self {
        AppState::Start
    }
}

pub struct ChatApp {
    state: AppState,
    nick: String,
    server_addr: String,
    private_key: String,
    client_port: u16,
    usernames: HashMap<u64, String>,
    messages: Vec<(u64, String, bool)>,
    chat_server: Option<ChatServer>,
    client: Option<RenetClient>,
    connection_error: Option<String>,
    text_input: String,
    message_id: u64,
    pending_messages: HashMap<u64, usize>,
}

impl Default for ChatApp {
    fn default() -> Self {
        Self {
            state: AppState::Start,
            nick: String::new(),
            server_addr: "127.0.0.1:5000".to_string(),
            private_key: "an example very very secret key.".to_string(),
            messages: vec![],
            chat_server: None,
            client: None,
            connection_error: None,
            text_input: String::new(),
            message_id: 0,
            client_port: 0,
            usernames: HashMap::new(),
            pending_messages: HashMap::new(),
        }
    }
}

impl ChatApp {
    fn draw_chat(&mut self, ctx: &egui::Context, _frame: &eframe::epi::Frame) {
        let Self { usernames, messages, text_input, client, chat_server, pending_messages, message_id, .. } = self;

        let client = client.as_mut().expect("client always exists when drawing chat.");

        egui::SidePanel::right("right_panel")
            .min_width(150.0)
            .default_width(200.0)
            .show(ctx, |ui| {
                if let Some(chat_server) = chat_server {
                    draw_host_commands(chat_server, ui);
                }
                ui.vertical_centered(|ui| {
                    ui.heading("Clients");
                });

                ui.separator();

                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    for username in usernames.values() {
                        ui.label(username);
                    }
                });
            });

        egui::TopBottomPanel::bottom("text_editor").show(ctx, |ui| {
            let send_message = ui.horizontal(|ui| {
                let response = ui.text_edit_singleline(text_input);

                let input_send = response.has_focus() && ui.input().key_pressed(egui::Key::Enter);
                let button_send = ui.button("Send").clicked();

                input_send || button_send
            });

            if send_message.inner {
                let message = bincode::options()
                    .serialize(&ClientMessages::Text(*message_id, text_input.clone()))
                    .unwrap();
                let index = messages.len();
                messages.push((client.client_id(), text_input.clone(), false));
                pending_messages.insert(*message_id, index);
                *message_id += 1;
                text_input.clear();
                if let Err(e) = client.send_message(0, message) {
                    log::error!("{}", e);
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                for (client, message, confirmed) in messages.iter() {
                    let label = if let Some(nick) = usernames.get(client) {
                        format!("{}: {}", nick, message)
                    } else if *client == 0 {
                        format!("Server: {}", message)
                    } else {
                        format!("unknown: {}", message)
                    };

                    let color = if *confirmed { ui.visuals().text_color() } else { ui.visuals().widgets.inactive.text_color() };
                    ui.colored_label(color, label);
                }
            });
        });
    }

    fn draw_start(&mut self, ctx: &egui::Context, _frame: &eframe::epi::Frame) {
        let Self {
            chat_server: server, nick, server_addr, state, client, connection_error, private_key, client_port, ..
        } = self;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_width(300.);
                ui.horizontal(|ui| {
                    ui.label("Nick:");
                    ui.text_edit_singleline(nick)
                });

                ui.horizontal(|ui| {
                    ui.label("Port:");
                    ui.add(egui::DragValue::new(client_port));
                });

                ui.horizontal(|ui| {
                    ui.label("Private Key:");
                    ui.text_edit_singleline(private_key);
                    private_key.truncate(NETCODE_KEY_BYTES);
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Server Addr:");
                    ui.text_edit_singleline(server_addr);
                });

                if ui.button("Connect").clicked() {
                    match server_addr.parse::<SocketAddr>() {
                        Err(_) => *connection_error = Some("Failed to parse server address".to_string()),
                        Ok(server_addr) => {
                            let mut key: [u8; NETCODE_KEY_BYTES] = [0; NETCODE_KEY_BYTES];
                            if private_key.as_bytes().len() != NETCODE_KEY_BYTES {
                                *connection_error = Some("Private key must have 32 bytes.".to_string());
                            } else {
                                key.copy_from_slice(private_key.as_bytes());
                                let remote_client = create_renet_client(*client_port, nick.clone(), server_addr, &key);

                                *state = AppState::Connecting;
                                *client = Some(remote_client);
                            }
                        }
                    }
                }

                ui.separator();

                if ui.button("Host").clicked() {
                    match server_addr.parse::<SocketAddr>() {
                        Err(_) => *connection_error = Some("Failed to parse server address".to_string()),
                        Ok(server_addr) => {
                            let mut key: [u8; NETCODE_KEY_BYTES] = [0; NETCODE_KEY_BYTES];
                            if private_key.as_bytes().len() != NETCODE_KEY_BYTES {
                                *connection_error = Some("Private key must have 32 bytes.".to_string());
                            } else {
                                key.copy_from_slice(private_key.as_bytes());
                                let remote_client = create_renet_client(*client_port, nick.clone(), server_addr, &key);
                                *state = AppState::Chat;
                                let chat_server = ChatServer::new(server_addr, &key);

                                *server = Some(chat_server);
                                *client = Some(remote_client);
                            }
                        }
                    }
                }

                ui.separator();

                if let Some(error) = connection_error {
                    ui.colored_label(Color32::RED, format!("Connection Error: {}", error));
                }
            });
        });
    }

    fn draw_connecting(&mut self, ctx: &egui::Context, _frame: &eframe::epi::Frame) {
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

    pub fn draw(&mut self, ctx: &egui::Context, frame: &eframe::epi::Frame) {
        match self.state {
            AppState::Chat => self.draw_chat(ctx, frame),
            AppState::Start => self.draw_start(ctx, frame),
            AppState::Connecting => self.draw_connecting(ctx, frame),
        }
    }

    pub fn update(&mut self, duration: Duration) {
        if let Some(chat_server) = self.chat_server.as_mut() {
            if let Err(e) = chat_server.update(duration) {
                error!("Failed updating server: {}", e);
                self.state = AppState::Start;
                self.connection_error = Some(e.to_string());
                self.chat_server = None;
                self.client = None;
            }
        }

        if let Some(chat_client) = self.client.as_mut() {
            if let Err(e) = chat_client.update(duration) {
                error!("{}", e);
            }
            if let Some(e) = chat_client.disconnected() {
                self.state = AppState::Start;
                self.connection_error = Some(e.to_string());
                self.chat_server = None;
                self.client = None;
            } else {
                while let Some(message) = chat_client.receive_message(0) {
                    let message: ServerMessages = bincode::options().deserialize(&message).unwrap();
                    match message {
                        ServerMessages::ClientConnected(id, nick) => {
                            self.usernames.insert(id, nick);
                        }
                        ServerMessages::ClientDisconnected(id) => {
                            self.usernames.remove(&id);
                            let message = format!("client {} disconnect", id);
                            self.messages.push((0, message, true));
                        }
                        ServerMessages::ClientMessage(client_id, text) => {
                            self.messages.push((client_id, text, true));
                        }
                        ServerMessages::MessageReceived(id) => {
                            if let Some(index) = self.pending_messages.remove(&id) {
                                if let Some(pending_message) = self.messages.get_mut(index) {
                                    pending_message.2 = true;
                                }
                            }
                        }
                        ServerMessages::InitClient { usernames } => {
                            if matches!(self.state, AppState::Connecting) {
                                self.connection_error = None;
                                self.state = AppState::Chat;
                                self.usernames = usernames;
                            }
                        }
                    }
                }
                if let Err(e) = chat_client.send_packets() {
                    error!("Error sending packets: {}", e);
                }
            }
        }
    }
}

fn create_renet_client(client_port: u16, username: String, server_addr: SocketAddr, private_key: &[u8; NETCODE_KEY_BYTES]) -> RenetClient {
    let client_addr = SocketAddr::from(([127, 0, 0, 1], client_port));
    let socket = UdpSocket::bind(client_addr).unwrap();
    let connection_config = RenetConnectionConfig::default();

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

fn draw_host_commands(chat_server: &mut ChatServer, ui: &mut Ui) {
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

    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        for client_id in chat_server.server.clients_id().into_iter() {
            ui.label(format!("Client {}", client_id));
            ui.horizontal(|ui| {
                if ui.button("disconnect").clicked() {
                    chat_server.server.disconnect(client_id);
                }
            });
        }
    });
}
