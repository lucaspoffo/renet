use eframe::{
    egui::{self, lerp, Color32, Pos2, Shape, Ui, Vec2},
    epi,
};

use log::error;
use renet::{
    client::{Client, RemoteClient},
    protocol::unsecure::UnsecureClientProtocol,
    remote_connection::ConnectionConfig,
    server::ConnectionPermission,
    UdpClient,
};

use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
};

use crate::server::ChatServer;
use crate::{channels_config, ClientMessages, ServerMessages};

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
    client_id: u64,
    clients: HashMap<u64, String>,
    messages: Vec<(u64, String)>,
    chat_server: Option<ChatServer>,
    client: Option<Box<dyn Client<u64>>>,
    connection_error: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    connection_permission: ConnectionPermission,
    text_input: String,
}

impl ChatApp {
    fn draw_chat(&mut self, ctx: &egui::CtxRef, _frame: &mut epi::Frame<'_>) {
        let Self {
            clients,
            messages,
            text_input,
            client,
            chat_server,
            connection_permission,
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
                    draw_host_commands(chat_server, connection_permission, ui);
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
                let message =
                    bincode::serialize(&ClientMessages::Text(text_input.clone())).unwrap();
                text_input.clear();
                client.send_message(0, message).unwrap();
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::auto_sized().show(ui, |ui| {
                for (client, message) in messages.iter() {
                    let label = if let Some(nick) = clients.get(client) {
                        format!("{}: {}", nick, message)
                    } else {
                        format!("unknown: {}", message)
                    };

                    ui.label(label);
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
            client_id,
            client,
            connection_error,
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
                    ui.label("ID:");
                    ui.add(egui::DragValue::new(client_id));
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Server Addr:");
                    ui.text_edit_singleline(server_addr)
                });

                if ui.button("Connect").clicked() {
                    match server_addr.parse::<SocketAddr>() {
                        Ok(addr) => {
                            *state = AppState::Connecting;

                            let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
                            let connection_config = ConnectionConfig::default();

                            let protocol = UnsecureClientProtocol::new(*client_id);
                            let transport = UdpClient::new(addr, protocol, socket);

                            let mut remote_client = RemoteClient::new(
                                *client_id,
                                transport,
                                channels_config(),
                                connection_config,
                            );

                            let init_message = ClientMessages::Init { nick: nick.clone() };
                            let init_message = bincode::serialize(&init_message).unwrap();
                            remote_client.send_message(0, init_message).unwrap();

                            *client = Some(Box::new(remote_client));
                        }
                        Err(e) => error!("{}", e),
                    }
                }

                ui.separator();

                if ui.button("Host").clicked() {
                    *state = AppState::Chat;
                    let addr = "127.0.0.1:0".parse().unwrap();
                    let mut chat_server = ChatServer::new(addr);
                    let mut local_client = chat_server.server.create_local_client(*client_id);

                    let init_message = ClientMessages::Init { nick: nick.clone() };
                    let init_message = bincode::serialize(&init_message).unwrap();
                    local_client.send_message(0, init_message).unwrap();

                    *client = Some(Box::new(local_client));
                    *server = Some(chat_server);
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

    pub fn update(&mut self) {
        if let Some(chat_server) = self.chat_server.as_mut() {
            if let Err(e) = chat_server.update() {
                error!("Failed updating server: {}", e);
                self.state = AppState::Start;
                self.connection_error = Some(Box::new(e));
                self.chat_server = None;
                self.client = None;
            } else {
                chat_server
                    .server
                    .set_connection_permission(self.connection_permission.clone());
            }
        }

        if let Some(chat_client) = self.client.as_mut() {
            if let Err(e) = chat_client.update() {
                error!("{}", e);
            }
            if let Some(e) = chat_client.connection_error() {
                self.state = AppState::Start;
                self.connection_error = Some(Box::new(e));
                self.chat_server = None;
                self.client = None;
            } else {
                if let Ok(Some(message)) = chat_client.receive_message(0) {
                    let message: ServerMessages = bincode::deserialize(&message).unwrap();
                    match message {
                        ServerMessages::ClientConnected(id, nick) => {
                            self.clients.insert(id, nick);
                        }
                        ServerMessages::ClientDisconnected(id) => {
                            self.clients.remove(&id);
                        }
                        ServerMessages::ClientMessage(nick, text) => {
                            self.messages.push((nick, text));
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
                chat_client.send_packets().unwrap();
            }
        }
    }
}

fn draw_host_commands(
    chat_server: &mut ChatServer,
    connection_permission: &mut ConnectionPermission,
    ui: &mut Ui,
) {
    ui.vertical_centered(|ui| {
        ui.heading("Server Commands");
    });

    ui.separator();
    let addr = chat_server.server.connection_id();
    ui.horizontal(|ui| {
        ui.label(format!("Address: {}", addr));
        let tooltip = "Click to copy the server address";
        if ui.button("📋").on_hover_text(tooltip).clicked() {
            ui.output().copied_text = addr.to_string();
        }
    });

    ui.separator();

    egui::ComboBox::from_label("Take your pick")
        .selected_text(format!("{:?}", connection_permission))
        .show_ui(ui, |ui| {
            ui.selectable_value(connection_permission, ConnectionPermission::All, "All");
            ui.selectable_value(
                connection_permission,
                ConnectionPermission::OnlyAllowed,
                "Only allowed",
            );
            ui.selectable_value(connection_permission, ConnectionPermission::None, "None");
        });

    egui::ScrollArea::auto_sized().show(ui, |ui| {
        for client_id in chat_server.server.get_clients_id().into_iter() {
            ui.label(format!("Client {}", client_id));
            ui.horizontal(|ui| {
                if ui.button("disconnect").clicked() {
                    chat_server.server.disconnect(&client_id);
                }

                if ui.button("allow").clicked() {
                    chat_server.server.allow_client(&client_id);
                }

                if ui.button("deny").clicked() {
                    chat_server.server.deny_client(&client_id);
                }
            });
        }
    });

    ui.separator();
    ui.label("Allowed Clients");
    egui::ScrollArea::auto_sized().show(ui, |ui| {
        for client_id in chat_server.server.allowed_clients().into_iter() {
            ui.label(format!("Client id {}", client_id));
        }
    });

    ui.separator();
    ui.label("Denied Clients");
    egui::ScrollArea::auto_sized().show(ui, |ui| {
        for client_id in chat_server.server.denied_clients().into_iter() {
            ui.label(format!("Client id {}", client_id));
        }
    });
}
