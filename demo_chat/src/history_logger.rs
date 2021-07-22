use log::{Level, Log};

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
/// Simple logger to keep the last records in an array
/// so we can display them in the interface. Also prints
/// to console.
pub struct HistoryLogger {
    records: Arc<Mutex<VecDeque<(Level, String)>>>,
    level: Level,
    capacity: usize,
}

impl HistoryLogger {
    pub fn new(capacity: usize, level: Level) -> Self {
        Self {
            records: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            level,
            capacity,
        }
    }

    pub fn init(self) {
        log::set_max_level(self.level.to_level_filter());
        log::set_boxed_logger(Box::new(self)).unwrap();
    }

    pub fn records(&self) -> Arc<Mutex<VecDeque<(Level, String)>>> {
        self.records.clone()
    }
}

impl Log for HistoryLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let mut records = self.records.lock().unwrap();
            let level = record.level().to_string();
            let target = if !record.target().is_empty() {
                record.target()
            } else {
                record.module_path().unwrap_or_default()
            };
            let message = format!("{:<5} [{}] {}", level, target, record.args());
            println!("{}", message);
            if records.len() >= self.capacity {
                records.pop_front();
            }
            records.push_back((record.level(), message));
        }
    }

    fn flush(&self) {}
}
