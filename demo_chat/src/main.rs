use log::Level;
use renet::channel::{ChannelConfig, ReliableOrderedChannelConfig};
use serde::{Deserialize, Serialize};

use history_logger::HistoryLogger;

use std::collections::HashMap;

mod client;
mod history_logger;
mod server;

fn channels_config() -> HashMap<u8, Box<dyn ChannelConfig>> {
    let mut channels_config: HashMap<u8, Box<dyn ChannelConfig>> = HashMap::new();

    let reliable_config = ReliableOrderedChannelConfig::default();
    channels_config.insert(0, Box::new(reliable_config));
    channels_config
}

#[derive(Debug, Serialize, Deserialize)]
enum ClientMessages {
    Text(String),
    Init { nick: String },
}

#[derive(Debug, Serialize, Deserialize)]
enum ServerMessages {
    ClientConnected(u64, String),
    ClientDisconnected(u64),
    ClientMessage(u64, String),
    InitClient { clients: HashMap<u64, String> },
}

fn main() {
    let logger = HistoryLogger::new(100, Level::Trace);
    let records = logger.records();
    logger.init();
    let app = client::App::with_records(records);
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(Box::new(app), native_options);
}
