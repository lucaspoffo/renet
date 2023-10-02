use futures::executor::block_on;
use std::{
    net::SocketAddr,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use log::{debug, info};
use renet::{ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
use renet_webtransport_server::{WebTransportConfig, WebTransportServer};
#[tokio::main]
async fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Info).init();
    for addr in tokio::net::lookup_host("localhost:4433").await.unwrap() {
        println!("socket address is {}", addr);
    }
    let future = server("127.0.0.1:4433".parse().unwrap());
    block_on(future);
}

async fn server(public_addr: SocketAddr) {
    let connection_config = ConnectionConfig::default();
    let mut server: RenetServer = RenetServer::new(connection_config);
    let server_config = WebTransportConfig {
        listen: public_addr,
        cert: PathBuf::from("renet_webtransport_server\\examples\\localhost.der"),
        key: PathBuf::from("renet_webtransport_server\\examples\\localhost_key.der"),
        max_clients: 10,
    };
    debug!("cert path: {:?}", server_config.cert.as_path());

    let mut transport = WebTransportServer::new(server_config).unwrap();
    debug!("transport created");
    let mut received_messages = vec![];
    let mut last_updated = Instant::now();

    loop {
        debug!("server tick");
        let now = Instant::now();
        let duration = now - last_updated;
        last_updated = now;

        server.update(duration);
        transport.update(&mut server);
        debug!("server update");

        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    info!("Client {} connected.", client_id);
                    server.send_message(client_id, DefaultChannel::Unreliable, b"Welcome to the server!".to_vec());
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    info!("Client {} disconnected: {}", client_id, reason);
                }
            }
        }

        for client_id in server.clients_id() {
            while let Some(message) = server.receive_message(client_id, DefaultChannel::Unreliable) {
                let message = String::from_utf8(message.into()).unwrap();
                let split = message.split_once(':').unwrap();
                let text = split.1;
                let username = split.0;
                info!("Client {} ({}) sent text: {}", username, client_id, text);
                let text = format!("{}: {}", username, text);
                received_messages.push(text);
            }
        }

        for text in received_messages.drain(..) {
            server.broadcast_message(DefaultChannel::Unreliable, text.as_bytes().to_vec());
        }

        transport.send_packets(&mut server);
        thread::sleep(Duration::from_millis(50));
    }
}
