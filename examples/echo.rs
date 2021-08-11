use renet::{
    channel::{ChannelConfig, ReliableOrderedChannelConfig},
    client::{Client, RemoteClient},
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol},
    remote_connection::ConnectionConfig,
    server::{ConnectionPermission, Server, ServerConfig, ServerEvent},
};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

fn main() {
    println!("Usage: server [SERVER_PORT] or client [SERVER_PORT] [CLIENT_ID]");
    let args: Vec<String> = std::env::args().collect();

    let exec_type = &args[1];
    match exec_type.as_str() {
        "client" => {
            let server_addr: SocketAddr = format!("127.0.0.1:{}", args[2]).parse().unwrap();
            let client_id: u64 = args[3].parse().unwrap();
            client(client_id, server_addr);
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

fn channels_config() -> HashMap<u8, Box<dyn ChannelConfig>> {
    let mut channels_config: HashMap<u8, Box<dyn ChannelConfig>> = HashMap::new();

    let reliable_config = ReliableOrderedChannelConfig::default();
    channels_config.insert(0, Box::new(reliable_config));
    channels_config
}

fn server(addr: SocketAddr) {
    let socket = UdpSocket::bind(addr).unwrap();
    let server_config = ServerConfig::default();
    let connection_config = ConnectionConfig::default();
    let mut server: Server<UnsecureServerProtocol<u64>> = Server::new(
        socket,
        server_config,
        connection_config,
        ConnectionPermission::All,
        channels_config(),
    )
    .unwrap();
    let mut received_messages = vec![];
    loop {
        server.update().unwrap();
        received_messages.clear();

        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected(id) => println!("Client {} connected.", id),
                ServerEvent::ClientDisconnected(id) => println!("Client {} disconnected.", id),
            }
        }

        for client_id in server.get_clients_id().into_iter() {
            while let Ok(Some(message)) = server.receive_message(client_id, 0) {
                let text = String::from_utf8(message).unwrap();
                println!("Client {} sent text: {}", client_id, text);
                received_messages.push(text);
            }
        }

        for text in received_messages.iter() {
            server.broadcast_message(0, text.as_bytes().to_vec());
        }

        server.send_packets();
        thread::sleep(Duration::from_millis(100));
    }
}

fn client(id: u64, server_addr: SocketAddr) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config = ConnectionConfig::default();
    let mut client = RemoteClient::new(
        id,
        socket,
        server_addr,
        channels_config(),
        UnsecureClientProtocol::new(id),
        connection_config,
    )
    .unwrap();
    let stdin_channel = spawn_stdin_channel();

    loop {
        client.update().unwrap();
        match stdin_channel.try_recv() {
            Ok(text) => client.send_message(0, text.as_bytes().to_vec()).unwrap(),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("Channel disconnected"),
        }

        while let Some(text) = client.receive_message(0).unwrap() {
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
