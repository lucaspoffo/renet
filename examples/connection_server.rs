use std::net::UdpSocket;
use renet::connection::{ConnectionError, Server, ServerConfig};
use alto_logger::TermLogger;
use std::thread::sleep;
use std::time::Duration;

fn main() -> Result<(), ConnectionError> {
    TermLogger::default().init().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:8080")?;
    let server_config = ServerConfig::new(8);
    let mut server = Server::new(socket, server_config)?;
    loop {
        server.update();
        sleep(Duration::from_millis(50));
    }
    Ok(())
}
