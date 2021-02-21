use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MICROS_PER_FRAME: u64 = 50000;

#[derive(Serialize, Deserialize, Debug)]
pub struct Message {
    pub time: SystemTime,
    pub tick: u64,
}

pub fn save_to_csv(hashmap: HashMap<u64, Duration>, filename: String) {
    let filename = &format!(
        "{}_{}.csv",
        filename,
        UNIX_EPOCH.elapsed().unwrap().as_millis()
    );
    let mut file = File::create(filename).unwrap();

    let mut values: Vec<(&u64, &Duration)> = hashmap.iter().collect();
    values.sort_by(|x, y| x.0.cmp(&y.0));
    for (&key, &dur) in values.iter() {
        file.write_all(format!("{}, {}\n", key, dur.as_micros()).as_bytes())
            .unwrap();
    }
}

pub fn save_to_csv_f64(hashmap: HashMap<u64, f64>, filename: String) {
    let filename = &format!(
        "{}_{}.csv",
        filename,
        UNIX_EPOCH.elapsed().unwrap().as_millis()
    );
    let mut file = File::create(filename).unwrap();

    let mut values: Vec<(&u64, &f64)> = hashmap.iter().collect();
    values.sort_by(|x, y| x.0.cmp(&y.0));
    for (&key, &value) in values.iter() {
        file.write_all(format!("{}, {}\n", key, value).as_bytes())
            .unwrap();
    }
}
