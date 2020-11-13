/*
use renet::connection::{RequestConnection, ConnectionError, ClientConnected};
use renet::RenetError;
use alto_logger::TermLogger;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::Duration;
fn main() -> Result<(), RenetError> {
    TermLogger::default().init().unwrap();
    let mut server_connection = get_connection()?;
    
    loop {
        server_connection.process_events()?;
        for payload in server_connection.received_payloads.iter() {
            log::trace!("Received payload from server:\n{:?}", payload);
        }
        server_connection.received_payloads.clear();
        server_connection.send_payload(b"Hello from client")?;
        log::trace!("Acks:\n{:?}", server_connection.endpoint.get_acks());
        sleep(Duration::from_millis(50));
    }
}

fn get_connection() -> Result<ClientConnected, ConnectionError> {
    let socket = UdpSocket::bind("127.0.0.1:8081")?;
    let mut request_connection = RequestConnection::new(0, socket, "127.0.0.1:8080".parse().unwrap())?;
    loop {
        match request_connection.update() {
            Ok(Some(connection)) => {
                log::debug!("Successfuly connected to the server!");
                return Ok(connection);
            },
            Err(ConnectionError::Denied) => {
                log::debug!("Connection denied to the server!");
                return Err(ConnectionError::Denied);
            },
            _ => {}

        }
        sleep(Duration::from_millis(50));
    }

}
*/
fn main() {}
