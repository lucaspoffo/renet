use std::net::UdpSocket;
use renet::connection::{ConnectionError, Server, ServerConfig};
use alto_logger::TermLogger;
use std::thread::sleep;
use std::time::Duration;

fn main() -> Result<(), ConnectionError> {
    TermLogger::default().init().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:8080")?;
    let server_config = ServerConfig::default();
    let mut server = Server::new(socket, server_config)?;
    loop {
        server.update();
        for (client_id, payload) in server.received_payloads.iter() {
            log::debug!("Received payload from client {}:\n{:?}", client_id, String::from_utf8_lossy(payload));
        }
        server.received_payloads.clear();
        sleep(Duration::from_millis(50));
    }
}
