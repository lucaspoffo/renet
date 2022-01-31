use client::ChatApp;
use eframe::{egui, epi};
use log::Level;
use renet::rechannel::error::DisconnectionReason;
use serde::{Deserialize, Serialize};

use history_logger::{HistoryLogger, LoggerApp};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

mod client;
mod history_logger;
mod server;

#[derive(Debug, Serialize, Deserialize)]
enum ClientMessages {
    Text(u64, String),
    Init { nick: String },
}

#[derive(Debug, Serialize, Deserialize)]
enum ServerMessages {
    ClientConnected(SocketAddr, String),
    ClientDisconnected(SocketAddr, DisconnectionReason),
    ClientMessage(SocketAddr, String),
    MessageReceived(u64),
    InitClient { clients: HashMap<SocketAddr, String> },
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

        Self {
            last_updated: Instant::now(),
            application: AppType::ChatApp,
            chat_app,
            logger_app,
        }
    }
}

impl epi::App for App {
    fn name(&self) -> &str {
        "Renet Chat"
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
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
