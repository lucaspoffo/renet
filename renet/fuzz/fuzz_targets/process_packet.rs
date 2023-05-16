#![no_main]

use libfuzzer_sys::fuzz_target;
use renet::{ConnectionConfig, RenetClient};

fuzz_target!(|data: &[u8]| {
    let mut connection = RenetClient::new(ConnectionConfig::default());
    connection.process_packet(data);
});
