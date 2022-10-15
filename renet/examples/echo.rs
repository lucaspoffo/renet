use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime},
};

use renet::{
    ClientAuthentication, DefaultChannel, RenetClient, RenetConnectionConfig, RenetServer, ServerAuthentication, ServerConfig, ServerEvent,
    UdpTransport, NETCODE_USER_DATA_BYTES,
};

// Helper struct to pass an username in the user data
struct Username(String);

impl Username {
    fn to_netcode_user_data(&self) -> [u8; NETCODE_USER_DATA_BYTES] {
        let mut user_data = [0u8; NETCODE_USER_DATA_BYTES];
        if self.0.len() > NETCODE_USER_DATA_BYTES - 8 {
            panic!("Username is too big");
        }
        user_data[0..8].copy_from_slice(&(self.0.len() as u64).to_le_bytes());
        user_data[8..self.0.len() + 8].copy_from_slice(self.0.as_bytes());

        user_data
    }

    fn from_user_data(user_data: &[u8; NETCODE_USER_DATA_BYTES]) -> Self {
        let mut buffer = [0u8; 8];
        buffer.copy_from_slice(&user_data[0..8]);
        let mut len = u64::from_le_bytes(buffer) as usize;
        len = len.min(NETCODE_USER_DATA_BYTES - 8);
        let data = user_data[8..len + 8].to_vec();
        let username = String::from_utf8(data).unwrap();
        Self(username)
    }
}

fn main() {
    println!("Usage: server [SERVER_PORT] or client [SERVER_PORT] [USER_NAME]");
    let args: Vec<String> = std::env::args().collect();

    let exec_type = &args[1];
    match exec_type.as_str() {
        "client" => {
            let server_addr: SocketAddr = format!("127.0.0.1:{}", args[2]).parse().unwrap();
            let username = Username(args[3].clone());
            client(server_addr, username);
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

const PROTOCOL_ID: u64 = 7;

fn server(addr: SocketAddr) {
    let socket = UdpSocket::bind(addr).unwrap();
    let transport = Box::new(UdpTransport::with_socket(socket).unwrap());
    let connection_config = RenetConnectionConfig::default();
    let server_config = ServerConfig::new(64, PROTOCOL_ID, addr, ServerAuthentication::Unsecure);
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let mut server: RenetServer = RenetServer::new(current_time, server_config, connection_config, transport);

    let mut usernames: HashMap<u64, String> = HashMap::new();
    let mut received_messages = vec![];
    let mut last_updated = Instant::now();

    loop {
        let now = Instant::now();
        server.update(now - last_updated).unwrap();
        last_updated = now;
        received_messages.clear();

        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected(id, user_data) => {
                    let username = Username::from_user_data(&user_data);
                    usernames.insert(id, username.0);
                    println!("Client {} connected.", id)
                }
                ServerEvent::ClientDisconnected(id) => {
                    println!("Client {} disconnected", id);
                    usernames.remove_entry(&id);
                }
            }
        }

        for client_id in server.clients_id().into_iter() {
            while let Some(message) = server.receive_message(client_id, DefaultChannel::Reliable) {
                let text = String::from_utf8(message).unwrap();
                let username = usernames.get(&client_id).unwrap();
                println!("Client {} ({}) sent text: {}", username, client_id, text);
                let text = format!("{}: {}", username, text);
                received_messages.push(text);
            }
        }

        for text in received_messages.iter() {
            server.broadcast_message(DefaultChannel::Reliable, text.as_bytes().to_vec());
        }

        server.send_packets().unwrap();
        thread::sleep(Duration::from_millis(50));
    }
}

fn client(server_addr: SocketAddr, username: Username) {
    let transport = Box::new(UdpTransport::new().unwrap());
    let connection_config = RenetConnectionConfig::default();
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let client_id = current_time.as_millis() as u64;
    let authentication = ClientAuthentication::Unsecure {
        server_addr,
        client_id,
        user_data: Some(username.to_netcode_user_data()),
        protocol_id: PROTOCOL_ID,
    };
    let mut client = RenetClient::new(current_time, connection_config, authentication, transport).unwrap();
    let stdin_channel = spawn_stdin_channel();

    let mut last_updated = Instant::now();
    loop {
        let now = Instant::now();
        client.update(now - last_updated).unwrap();
        last_updated = now;
        if client.is_connected() {
            match stdin_channel.try_recv() {
                Ok(text) => client.send_message(DefaultChannel::Reliable, text.as_bytes().to_vec()),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => panic!("Channel disconnected"),
            }

            while let Some(text) = client.receive_message(DefaultChannel::Reliable) {
                let text = String::from_utf8(text).unwrap();
                println!("{}", text);
            }
        }

        client.send_packets().unwrap();
        thread::sleep(Duration::from_millis(50));
    }
}

fn spawn_stdin_channel() -> Receiver<String> {
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || loop {
        let mut buffer = String::new();
        std::io::stdin().read_line(&mut buffer).unwrap();
        tx.send(buffer.trim_end().to_string()).unwrap();
    });
    rx
}
