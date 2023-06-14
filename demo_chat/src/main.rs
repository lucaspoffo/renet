use client::ChatApp;
use eframe::{egui, App};
use renet::transport::NETCODE_USER_DATA_BYTES;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

mod client;
mod server;
mod ui;

const PROTOCOL_ID: u64 = 27;

// Helper struct to pass an username in user data inside the ConnectToken
pub struct Username(pub String);

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

impl Username {
    pub fn to_netcode_user_data(&self) -> [u8; NETCODE_USER_DATA_BYTES] {
        let mut user_data = [0u8; NETCODE_USER_DATA_BYTES];
        if self.0.len() > NETCODE_USER_DATA_BYTES - 8 {
            panic!("Username is too big");
        }
        user_data[0] = self.0.len() as u8;
        user_data[1..self.0.len() + 1].copy_from_slice(self.0.as_bytes());

        user_data
    }

    pub fn from_user_data(user_data: &[u8; NETCODE_USER_DATA_BYTES]) -> Self {
        let mut len = user_data[0] as usize;
        len = len.min(NETCODE_USER_DATA_BYTES - 1);
        let data = user_data[1..len + 1].to_vec();
        let username = String::from_utf8(data).unwrap_or("unknown".to_string());
        Self(username)
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
