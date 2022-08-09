use bincode::Options;
use eframe::{
    egui::{self, lerp, Color32, Layout, Pos2, Ui, Vec2, Widget},
    epaint::PathShape,
};
use egui_extras::{Size, TableBuilder};
use matcher::{LobbyListing, RequestConnection};
use renet::DefaultChannel;

use std::{collections::HashMap, sync::mpsc};

use crate::{client::connect_token_request, ClientMessages};
use crate::{
    client::{AppState, UiState},
    server::ChatServer,
};

pub fn draw_lobby_list(ui: &mut Ui, lobby_list: Vec<LobbyListing>) -> Option<(u64, bool)> {
    ui.separator();
    ui.heading("Lobby list");
    if lobby_list.is_empty() {
        ui.label("No lobbies available");
        return None;
    }

    let mut connect_server_id = None;
    TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right())
        .column(Size::exact(12.))
        .column(Size::remainder())
        .column(Size::exact(40.))
        .column(Size::exact(60.))
        .header(12.0, |mut header| {
            header.col(|_| {});
            header.col(|ui| {
                ui.label("Name");
            });
        })
        .body(|mut body| {
            for lobby in lobby_list.iter() {
                body.row(30., |mut row| {
                    row.col(|ui| {
                        if lobby.is_protected {
                            ui.label("🔒");
                        }
                    });

                    row.col(|ui| {
                        ui.label(&lobby.name);
                    });

                    row.col(|ui| {
                        ui.label(format!("{}/{}", lobby.current_clients, lobby.max_clients));
                    });

                    row.col(|ui| {
                        if ui.button("connect").clicked() {
                            connect_server_id = Some((lobby.id, lobby.is_protected));
                        }
                    });
                });
            }
        });

    connect_server_id
}

pub fn draw_loader(ctx: &egui::Context) {
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

pub fn draw_host_commands(ui: &mut Ui, chat_server: &mut ChatServer) {
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

pub fn draw_main_screen(ui_state: &mut UiState, state: &mut AppState, lobby_list: Vec<LobbyListing>, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        egui::Area::new("buttons")
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
                        ui.label("Lobby name:");
                        ui.text_edit_singleline(&mut ui_state.lobby_name)
                    });

                    ui.horizontal(|ui| {
                        ui.label("Lobby password:").on_hover_text("Password can be empty");
                        egui::TextEdit::singleline(&mut ui_state.password).password(true).ui(ui)
                    });

                    ui.vertical_centered_justified(|ui| {
                        if ui.button("Host").clicked() {
                            if ui_state.username.is_empty() || ui_state.lobby_name.is_empty() {
                                ui_state.error = Some("Nick or Lobby name can't be empty".to_owned());
                            } else {
                                let server =
                                    ChatServer::new(ui_state.lobby_name.clone(), ui_state.username.clone(), ui_state.password.clone());
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

                    if let Some((connect_server_id, is_protected)) = draw_lobby_list(ui, lobby_list) {
                        if ui_state.username.is_empty() {
                            ui_state.error = Some("Nick can't be empty".to_owned());
                        } else if is_protected && ui_state.password.is_empty() {
                            ui_state.error = Some("Lobby is protected, please insert a password".to_owned());
                        } else {
                            let (sender, receiver) = mpsc::channel();
                            let password = if is_protected { Some(ui_state.password.clone()) } else { None };
                            let request_connection = RequestConnection {
                                username: ui_state.username.clone(),
                                password,
                            };

                            std::thread::spawn(move || {
                                if let Err(e) = connect_token_request(connect_server_id, request_connection, sender) {
                                    log::error!("Failed to get connect token for server {}: {}", connect_server_id, e);
                                }
                            });

                            *state = AppState::RequestingToken { token: receiver };
                        }
                    }
                });
            });
    });
}

pub fn draw_chat(ui_state: &mut UiState, state: &mut AppState, usernames: HashMap<u64, String>, ctx: &egui::Context) {
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
                chat_server.server.disconnect_clients();
            }
            AppState::ClientChat { client, .. } => {
                client.disconnect();
            }
            _ => {}
        }
        *state = AppState::main_screen();
        ui_state.error = None;
        return;
    }

    egui::TopBottomPanel::bottom("text_editor").show(ctx, |ui| {
        let send_message = ui.horizontal(|ui| {
            let response = ui.text_edit_singleline(&mut ui_state.text_input);

            // Pressing enter makes we lose focus
            let input_send = response.lost_focus() && ui.input().key_pressed(egui::Key::Enter);
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
                    chat_server.receive_message(1, text);
                }
                AppState::ClientChat { client, .. } => {
                    let message = bincode::options().serialize(&ClientMessages::Text(text)).unwrap();
                    client.send_message(DefaultChannel::Reliable, message);
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
