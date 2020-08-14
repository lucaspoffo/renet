use renet::connection::{RequestConnection, ConnectionError};
use alto_logger::TermLogger;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::Duration;

fn main() -> Result<(), ConnectionError> {
    TermLogger::default().init().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:8081")?;
    let mut request_connection = RequestConnection::new(socket, "127.0.0.1:8080".parse().unwrap())?;

    loop {
        match request_connection.update() {
            Ok(Some(connection)) => {
                log::debug!("Successfuly connected to the server!");
                break;
            },
            Err(ConnectionError::Denied) => {
                log::debug!("Connection denied to the server!");
                break;
            },
            _ => {}

        }
        sleep(Duration::from_millis(50));
    }
    Ok(())
}
