use bincode::Options;
use eframe::{
    egui::{self, lerp, Color32, Layout, Pos2, Ui, Vec2},
    epaint::{PathShape, PathStroke},
};
use renet::{
    transport::{ClientAuthentication, NetcodeClientTransport},
    ClientId, ConnectionConfig, DefaultChannel, RenetClient,
};

use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    time::SystemTime,
};

use crate::{
    client::{AppState, UiState},
    server::{ChatServer, HOST_CLIENT_ID, SYSTEM_MESSAGE_CLIENT_ID},
};
use crate::{ClientMessages, Username, PROTOCOL_ID};

pub fn draw_loader(ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        // Taken from egui progress bar widget
        let n_points = 20;
        let time = ui.input(|input| input.time);
        let start_angle = time * 360f64.to_radians();
        let end_angle = start_angle + 240f64.to_radians() * time.sin();
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
            stroke: PathStroke::new(2.0, ui.visuals().text_color()),
        });
    });
}

pub fn draw_host_commands(ui: &mut Ui, chat_server: &mut ChatServer) {
    ui.vertical_centered(|ui| {
        ui.heading("Server Commands");
    });

    ui.separator();
    for server_addr in chat_server.transport.addresses() {
        ui.horizontal(|ui| {
            ui.label(format!("Address: {}", server_addr));
            let tooltip = "Click to copy the server address";
            if ui.button("📋").on_hover_text(tooltip).clicked() {
                ui.output_mut(|output| output.copied_text = server_addr.to_string());
            }
        });
    }

    ui.separator();

    egui::ScrollArea::vertical().id_source("host_commands_scroll").show(ui, |ui| {
        for client_id in chat_server.server.clients_id() {
            ui.horizontal(|ui| {
                ui.label(format!("Client {}", client_id));
                if ui.button("X").on_hover_text("Disconnect client").clicked() {
                    chat_server.server.disconnect(client_id);
                }
            });
        }
    });
}

pub fn draw_main_screen(ui_state: &mut UiState, state: &mut AppState, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        egui::Area::new("buttons".into())
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(300.);
                ui.set_height(300.);
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Nick:");
                        ui.text_edit_singleline(&mut ui_state.username)
                    });

                    ui.horizontal(|ui| {
                        ui.label("Server Addr:");
                        ui.text_edit_singleline(&mut ui_state.server_addr)
                    });

                    ui.vertical_centered_justified(|ui| {
                        if ui.button("Connect").clicked() {
                            match ui_state.server_addr.parse::<SocketAddr>() {
                                Err(_) => ui_state.error = Some("Failed to parse server address".to_string()),
                                Ok(server_addr) => {
                                    if ui_state.username.is_empty() {
                                        ui_state.error = Some("Nick can't be empty".to_owned());
                                    } else {
                                        let (client, transport) = create_renet_client(ui_state.username.clone(), server_addr);

                                        *state = AppState::ClientChat {
                                            visualizer: Box::default(),
                                            client: Box::new(client),
                                            transport: Box::new(transport),
                                            messages: vec![],
                                            usernames: HashMap::new(),
                                        };
                                    }
                                }
                            }
                        }
                    });

                    ui.vertical_centered_justified(|ui| {
                        if ui.button("Host").clicked() {
                            if ui_state.username.is_empty() {
                                ui_state.error = Some("Nick can't be empty".to_owned());
                            } else {
                                let server = ChatServer::new(ui_state.username.clone());
                                *state = AppState::HostChat {
                                    chat_server: Box::new(server),
                                };
                            }
                        }
                    });

                    if let Some(error) = &ui_state.error {
                        ui.separator();
                        ui.colored_label(Color32::RED, format!("Error: {}", error));
                    }
                });
            });
    });
}

pub fn draw_chat(ui_state: &mut UiState, state: &mut AppState, usernames: HashMap<ClientId, String>, ctx: &egui::Context) {
    if ui_state.show_network_info {
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
            ui.checkbox(&mut ui_state.show_network_info, "Show Network Graphs");

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
                chat_server.server.disconnect_all();
            }
            AppState::ClientChat { client, .. } => {
                client.disconnect();
            }
            _ => {}
        }
        *state = AppState::MainScreen;
        ui_state.error = None;
        return;
    }

    egui::TopBottomPanel::bottom("text_editor").show(ctx, |ui| {
        let send_message = ui.horizontal(|ui| {
            let response = ui.text_edit_singleline(&mut ui_state.text_input);

            // Pressing enter makes we lose focus
            let input_send = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let button_send = ui.button("Send").clicked();

            let send_message = input_send || button_send;
            if send_message {
                response.request_focus();
            }

            send_message
        });

        if send_message.inner && !ui_state.text_input.is_empty() {
            let text = ui_state.text_input.clone();
            match state {
                AppState::HostChat { chat_server } => {
                    // Simulate receiving a message sent by the host
                    chat_server.receive_message(HOST_CLIENT_ID, text);
                }
                AppState::ClientChat { client, .. } => {
                    let message = bincode::options().serialize(&ClientMessages::Text(text)).unwrap();
                    client.send_message(DefaultChannel::ReliableOrdered, message);
                }
                _ => unreachable!(),
            };
            ui_state.text_input.clear();
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
                    } else if message.client_id == SYSTEM_MESSAGE_CLIENT_ID {
                        format!("Server: {}", message.text)
                    } else {
                        format!("unknown: {}", message.text)
                    };

                    ui.label(text);
                }
            });
    });
}

fn create_renet_client(username: String, server_addr: SocketAddr) -> (RenetClient, NetcodeClientTransport) {
    let connection_config = ConnectionConfig::default();
    let client = RenetClient::new(connection_config);

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let client_id = current_time.as_millis() as u64;
    let authentication = ClientAuthentication::Unsecure {
        server_addr,
        client_id,
        user_data: Some(Username(username).to_netcode_user_data()),
        protocol_id: PROTOCOL_ID,
    };

    let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

    (client, transport)
}
