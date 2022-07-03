#![no_main]
use libfuzzer_sys::fuzz_target;

use rechannel::remote_connection::{ConnectionConfig, RemoteConnection};
use std::time::Duration;

fuzz_target!(|data: &[u8]| {
    let mut connection = RemoteConnection::new(Duration::ZERO, ConnectionConfig::default());
    connection.process_packet(data).ok();
});
