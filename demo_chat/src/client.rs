use bincode::Options;
use eframe::{
    egui::{self, lerp, Color32, Pos2, Shape, Ui, Vec2},
    epi,
};
use log::error;
use renet_udp::{
    client::UdpClient,
    renet::{error::RenetError, remote_connection::ConnectionConfig},
};

use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use crate::server::ChatServer;
use crate::{reliable_channels_config, ClientMessages, ServerMessages};

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

#[derive(Default)]
pub struct ChatApp {
    state: AppState,
    nick: String,
    server_addr: String,
    connected_server_addr: Option<SocketAddr>,
    client_port: u16,
    clients: HashMap<SocketAddr, String>,
    messages: Vec<(SocketAddr, String, bool)>,
    chat_server: Option<ChatServer>,
    client: Option<UdpClient>,
    connection_error: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    text_input: String,
    message_id: u64,
    pending_messages: HashMap<u64, usize>,
}

impl ChatApp {
    fn draw_chat(&mut self, ctx: &egui::CtxRef, _frame: &mut epi::Frame<'_>) {
        let Self {
            clients,
            messages,
            text_input,
            client,
            chat_server,
            pending_messages,
            message_id,
            ..
        } = self;

        let client = client
            .as_mut()
            .expect("Client always exists when drawing chat.");

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

                egui::ScrollArea::auto_sized().show(ui, |ui| {
                    for client in clients.values() {
                        ui.label(client);
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
                messages.push((client.id(), text_input.clone(), false));
                pending_messages.insert(*message_id, index);
                *message_id += 1;
                text_input.clear();
                if let Err(e) = client.send_reliable_message(0, message) {
                    log::error!("{}", e);
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::auto_sized().show(ui, |ui| {
                for (client, message, confirmed) in messages.iter() {
                    let label = if let Some(nick) = clients.get(client) {
                        format!("{}: {}", nick, message)
                    } else {
                        format!("unknown: {}", message)
                    };

                    let color = if *confirmed {
                        Color32::WHITE
                    } else {
                        Color32::GRAY
                    };
                    ui.colored_label(color, label);
                }
            });
        });
    }

    fn draw_start(&mut self, ctx: &egui::CtxRef, _frame: &mut epi::Frame<'_>) {
        let Self {
            chat_server: server,
            nick,
            server_addr,
            state,
            client,
            connection_error,
            client_port,
            connected_server_addr,
            ..
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

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Server Addr:");
                    ui.text_edit_singleline(server_addr)
                });

                if ui.button("Connect").clicked() {
                    match server_addr.parse::<SocketAddr>() {
                        Ok(server_addr) => {
                            *state = AppState::Connecting;
                            let client_addr = SocketAddr::from(([127, 0, 0, 1], *client_port));
                            let socket = UdpSocket::bind(client_addr).unwrap();
                            let connection_config = ConnectionConfig::default();

                            let mut remote_client = UdpClient::new(
                                socket,
                                server_addr,
                                connection_config,
                                reliable_channels_config(),
                            )
                            .unwrap();

                            let init_message = ClientMessages::Init { nick: nick.clone() };
                            let init_message = bincode::options().serialize(&init_message).unwrap();
                            if let Err(e) = remote_client.send_reliable_message(0, init_message) {
                                log::error!("{}", e);
                            }

                            *client = Some(remote_client);
                            *connected_server_addr = Some(server_addr);
                        }
                        Err(e) => error!("{}", e),
                    }
                }

                ui.separator();

                if ui.button("Host").clicked() {
                    *state = AppState::Chat;
                    let addr = "127.0.0.1:5000".parse().unwrap();
                    let chat_server = ChatServer::new(addr);

                    let client_addr = SocketAddr::from(([127, 0, 0, 1], *client_port));
                    let socket = UdpSocket::bind(client_addr).unwrap();
                    let connection_config = ConnectionConfig::default();

                    let mut remote_client =
                        UdpClient::new(socket, addr, connection_config, reliable_channels_config())
                            .unwrap();

                    let init_message = ClientMessages::Init { nick: nick.clone() };
                    let init_message = bincode::options().serialize(&init_message).unwrap();
                    if let Err(e) = remote_client.send_reliable_message(0, init_message) {
                        log::error!("{}", e);
                    }

                    *server = Some(chat_server);
                    *client = Some(remote_client);
                    *connected_server_addr = Some(addr);
                }

                ui.separator();

                if let Some(error) = connection_error {
                    ui.colored_label(Color32::RED, format!("Connection Error: {}", error));
                }
            });
        });
    }

    fn draw_connecting(&mut self, ctx: &egui::CtxRef, _frame: &mut epi::Frame<'_>) {
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
                    center
                        + circle_radius * Vec2::new(cos as f32, sin as f32)
                        + Vec2::new(circle_radius, 0.0)
                })
                .collect();
            ui.painter().add(Shape::Path {
                points,
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: egui::Stroke::new(2.0, Color32::WHITE),
            });
        });
    }

    pub fn draw(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
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
                self.connection_error = Some(Box::new(e));
                self.connected_server_addr = None;
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
                self.connection_error = Some(Box::new(RenetError::ClientDisconnected(e)));
                self.connected_server_addr = None;
                self.chat_server = None;
                self.client = None;
            } else {
                if let Some(message) = chat_client.receive_reliable_message(0) {
                    let message: ServerMessages = bincode::options().deserialize(&message).unwrap();
                    match message {
                        ServerMessages::ClientConnected(id, nick) => {
                            self.clients.insert(id, nick);
                        }
                        ServerMessages::ClientDisconnected(id, reason) => {
                            self.clients.remove(&id);
                            let message = format!("client {} disconnect: {}", id, reason);
                            self.messages.push((
                                self.connected_server_addr.unwrap(),
                                message,
                                true,
                            ));
                        }
                        ServerMessages::ClientMessage(nick, text) => {
                            self.messages.push((nick, text, true));
                        }
                        ServerMessages::MessageReceived(id) => {
                            if let Some(index) = self.pending_messages.remove(&id) {
                                if let Some(pending_message) = self.messages.get_mut(index) {
                                    pending_message.2 = true;
                                }
                            }
                        }
                        ServerMessages::InitClient { clients } => {
                            if matches!(self.state, AppState::Connecting) {
                                self.connection_error = None;
                                self.state = AppState::Chat;
                                self.clients = clients;
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

fn draw_host_commands(chat_server: &mut ChatServer, ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.heading("Server Commands");
    });

    ui.separator();
    if let Ok(addr) = chat_server.server.addr() {
        ui.horizontal(|ui| {
            ui.label(format!("Address: {}", addr));
            let tooltip = "Click to copy the server address";
            if ui.button("📋").on_hover_text(tooltip).clicked() {
                ui.output().copied_text = addr.to_string();
            }
        });

        ui.separator();
    }

    egui::ScrollArea::auto_sized().show(ui, |ui| {
        for client_id in chat_server.server.clients_id().into_iter() {
            ui.label(format!("Client {}", client_id));
            ui.horizontal(|ui| {
                if ui.button("disconnect").clicked() {
                    chat_server.server.disconnect(&client_id);
                }
            });
        }
    });
}
