use client::ChatApp;
use eframe::{egui, epi};
use log::Level;
use renet::NETCODE_USER_DATA_BYTES;
use serde::{Deserialize, Serialize};

use history_logger::{HistoryLogger, LoggerApp};

use std::collections::HashMap;
use std::time::Instant;

mod client;
mod history_logger;
mod server;

// Helper struct to pass an username in user data inside the ConnectToken
struct Username(String);

#[derive(Debug, Serialize, Deserialize)]
enum ClientMessages {
    Text(u64, String),
}

#[derive(Debug, Serialize, Deserialize)]
enum ServerMessages {
    ClientConnected(u64, String),
    ClientDisconnected(u64),
    ClientMessage(u64, String),
    MessageReceived(u64),
    InitClient { usernames: HashMap<u64, String> },
}

fn main() {
    let app = App::new();
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(Box::new(app), native_options);
}

#[derive(Debug, PartialEq, Eq)]
enum AppType {
    ChatApp,
    LoggerApp,
}

// Application to choose between chat or log.
struct App {
    application: AppType,
    chat_app: ChatApp,
    logger_app: LoggerApp,
    last_updated: Instant,
}

impl App {
    pub fn new() -> Self {
        let logger = HistoryLogger::new(100, Level::Trace);
        let records = logger.records();
        logger.init();

        let logger_app = LoggerApp::new(records);
        let chat_app = ChatApp::default();

        Self { last_updated: Instant::now(), application: AppType::ChatApp, chat_app, logger_app }
    }
}

impl epi::App for App {
    fn name(&self) -> &str {
        "Renet Chat"
    }

    fn update(&mut self, ctx: &egui::Context, frame: &eframe::epi::Frame) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(self.application == AppType::ChatApp, "Chat App").clicked() {
                    self.application = AppType::ChatApp;
                }

                if ui.selectable_label(self.application == AppType::LoggerApp, "Logger App").clicked() {
                    self.application = AppType::LoggerApp;
                }
            });
        });

        match self.application {
            AppType::ChatApp => self.chat_app.draw(ctx, frame),
            AppType::LoggerApp => self.logger_app.draw(ctx, frame),
        }

        let now = Instant::now();
        self.chat_app.update(now - self.last_updated);
        self.last_updated = now;
        ctx.request_repaint();
    }
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
