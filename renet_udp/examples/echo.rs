use renet_udp::{
    client::UdpClient,
    renet::{channel::reliable::ReliableChannelConfig, remote_connection::ConnectionConfig},
    server::{ServerEvent, UdpServer},
};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;
use std::{
    net::{SocketAddr, UdpSocket},
    time::Instant,
};

fn main() {
    println!("Usage: server [SERVER_PORT] or client [SERVER_PORT]");
    let args: Vec<String> = std::env::args().collect();

    let exec_type = &args[1];
    match exec_type.as_str() {
        "client" => {
            let server_addr: SocketAddr = format!("127.0.0.1:{}", args[2]).parse().unwrap();
            client(server_addr);
        }
        "server" => {
            let server_addr: SocketAddr = format!("127.0.0.1:{}", args[2]).parse().unwrap();
            server(server_addr);
        }
        _ => {
            println!("Invalid argument, first one must be \"client\" or \"server\".");
        }
    }
}

fn reliable_channels_config() -> Vec<ReliableChannelConfig> {
    let reliable_config = ReliableChannelConfig::default();
    vec![reliable_config]
}

fn server(addr: SocketAddr) {
    let socket = UdpSocket::bind(addr).unwrap();
    let connection_config = ConnectionConfig::default();
    let mut server: UdpServer = UdpServer::new(64, connection_config, reliable_channels_config(), socket).unwrap();
    let mut received_messages = vec![];
    let mut last_updated = Instant::now();
    loop {
        server.update(Instant::now() - last_updated).unwrap();
        last_updated = Instant::now();
        received_messages.clear();

        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected(id) => println!("Client {} connected.", id),
                ServerEvent::ClientDisconnected(id, reason) => {
                    println!("Client {} disconnected: {}.", id, reason)
                }
            }
        }

        for client_id in server.clients_id().iter() {
            while let Some(message) = server.receive_reliable_message(client_id, 0) {
                let text = String::from_utf8(message).unwrap();
                println!("Client {} sent text: {}", client_id, text);
                received_messages.push(text);
            }
        }

        for text in received_messages.iter() {
            server.broadcast_reliable_message(0, text.as_bytes().to_vec());
        }

        server.send_packets().unwrap();
        thread::sleep(Duration::from_millis(100));
    }
}

fn client(server_addr: SocketAddr) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config = ConnectionConfig::default();
    let mut client = UdpClient::new(socket, server_addr, connection_config, reliable_channels_config()).unwrap();
    let stdin_channel = spawn_stdin_channel();

    let mut last_updated = Instant::now();
    loop {
        client.update(Instant::now() - last_updated).unwrap();
        last_updated = Instant::now();
        match stdin_channel.try_recv() {
            Ok(text) => client.send_reliable_message(0, text.as_bytes().to_vec()).unwrap(),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("Channel disconnected"),
        }

        while let Some(text) = client.receive_reliable_message(0) {
            let text = String::from_utf8(text).unwrap();
            println!("Message from server: {}", text);
        }

        client.send_packets().unwrap();
        thread::sleep(Duration::from_millis(100));
    }
}

fn spawn_stdin_channel() -> Receiver<String> {
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || loop {
        let mut buffer = String::new();
        std::io::stdin().read_line(&mut buffer).unwrap();
        tx.send(buffer).unwrap();
    });
    rx
}
