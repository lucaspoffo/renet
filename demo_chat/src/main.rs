use client::ChatApp;
use eframe::{egui, App};
use renet::{ChannelConfig, ReliableChannelConfig, NETCODE_USER_DATA_BYTES};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

mod client;
mod server;

// Helper struct to pass an username in user data inside the ConnectToken
struct Username(String);

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

#[derive(Debug)]
pub enum Channels {
    Reliable,
}

impl Channels {
    pub fn id(&self) -> u8 {
        match self {
            Channels::Reliable => 0,
        }
    }
}

pub fn channels_config() -> Vec<ChannelConfig> {
    let reliable_channel = ChannelConfig::Reliable(ReliableChannelConfig {
        channel_id: Channels::Reliable.id(),
        ..Default::default()
    });

    vec![reliable_channel]
}

impl Username {
    fn to_netcode_user_data(&self) -> [u8; NETCODE_USER_DATA_BYTES] {
        let mut user_data = [0u8; NETCODE_USER_DATA_BYTES];
        if self.0.len() > NETCODE_USER_DATA_BYTES - 8 {
            panic!("Username is too big");
        }
        user_data[0..8].copy_from_slice(&(self.0.len() as u64).to_le_bytes());
        user_data[8..self.0.len() + 8].copy_from_slice(self.0.as_bytes());

        user_data
    }

    fn from_user_data(user_data: &[u8; NETCODE_USER_DATA_BYTES]) -> Self {
        let mut buffer = [0u8; 8];
        buffer.copy_from_slice(&user_data[0..8]);
        let mut len = u64::from_le_bytes(buffer) as usize;
        len = len.min(NETCODE_USER_DATA_BYTES - 8);
        let data = user_data[8..len + 8].to_vec();
        let username = String::from_utf8(data).unwrap();
        Self(username)
    }
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

fn main() {
    env_logger::init();
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Renet Demo Chat",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Box::new(ChatApp::default())
        }),
    );
}
