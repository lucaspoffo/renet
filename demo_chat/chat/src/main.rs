use client::ChatApp;
use eframe::{egui, App};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

mod client;
mod lobby_status;
mod server;
mod ui;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    client_id: u64,
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum ClientMessages {
    Text(String),
}

#[derive(Debug, Serialize, Deserialize)]
enum ServerMessages {
    ClientConnected { client_id: u64, username: String },
    ClientDisconnected { client_id: u64 },
    ClientMessage(Message),
    InitClient { usernames: HashMap<u64, String> },
}

impl Message {
    fn new(client_id: u64, text: String) -> Self {
        Self { client_id, text }
    }
}

impl App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
        self.update_chat();
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Renet Demo Chat",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Box::<ChatApp>::default()
        }),
    )
}
