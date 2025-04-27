use renetcode::{
    ClientAuthentication, ConnectToken, NetcodeClient, NetcodeServer, ServerAuthentication, ServerConfig, ServerResult, NETCODE_KEY_BYTES,
    NETCODE_MAX_PACKET_BYTES, NETCODE_USER_DATA_BYTES,
};
use std::time::Duration;
use std::{collections::HashMap, thread};
use std::{
    net::{SocketAddr, UdpSocket},
    time::Instant,
};
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{SystemTime, UNIX_EPOCH},
};

// Unique id used for a version of your game
const PROTOCOL_ID: u64 = 123456789;

// Helper struct to pass an username in user data inside the ConnectToken
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
    let private_key = b"an example very very secret key."; // 32-bytes

    let exec_type = &args[1];
    match exec_type.as_str() {
        "client" => {
            let server_addr: SocketAddr = format!("127.0.0.1:{}", args[2]).parse().unwrap();
            let username = Username(args[3].clone());
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            println!("Stating connecting at {:?} with username {}", now, username.0,);
            let client_id = now.as_millis() as u64;
            let connect_token = ConnectToken::generate(
                now,
                PROTOCOL_ID,
                300,
                client_id,
                15,
                vec![server_addr],
                Some(&username.to_netcode_user_data()),
                private_key,
            )
            .unwrap();
            let auth = ClientAuthentication::Secure { connect_token };
            client(auth);
        }
        "server" => {
            let server_addr: SocketAddr = format!("127.0.0.1:{}", args[2]).parse().unwrap();
            server(server_addr, *private_key);
        }
        _ => {
            println!("Invalid argument, first one must be \"client\" or \"server\".");
        }
    }
}

fn handle_server_result(
    server_result: ServerResult,
    socket: &UdpSocket,
    received_messages: &mut Vec<String>,
    usernames: &mut HashMap<u64, String>,
) {
    match server_result {
        ServerResult::Payload { client_id, payload } => {
            let text = String::from_utf8(payload.to_vec()).unwrap();
            let username = usernames.get(&client_id).unwrap();
            println!("Client {} ({}) sent message {:?}.", username, client_id, text);
            let text = format!("{}: {}", username, text);
            received_messages.push(text);
        }
        ServerResult::PacketToSend { payload, addr } => {
            socket.send_to(payload, addr).unwrap();
        }
        ServerResult::ClientConnected {
            client_id,
            user_data,
            payload,
            addr,
        } => {
            let username = Username::from_user_data(&user_data);
            println!("Client {} with id {} connected.", username.0, client_id);
            usernames.insert(client_id, username.0);
            socket.send_to(payload, addr).unwrap();
        }
        ServerResult::ClientDisconnected { client_id, addr, payload } => {
            println!("Client {} disconnected.", client_id);
            usernames.remove_entry(&client_id);
            if let Some(payload) = payload {
                socket.send_to(payload, addr).unwrap();
            }
        }
        ServerResult::None => {}
    }
}

fn server(addr: SocketAddr, private_key: [u8; NETCODE_KEY_BYTES]) {
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let config = ServerConfig {
        current_time,
        max_clients: 16,
        protocol_id: PROTOCOL_ID,
        public_addresses: vec![addr],
        authentication: ServerAuthentication::Secure { private_key },
    };
    let mut server: NetcodeServer = NetcodeServer::new(config);
    let udp_socket = UdpSocket::bind(addr).unwrap();
    udp_socket.set_nonblocking(true).unwrap();
    let mut received_messages = vec![];
    let mut last_updated = Instant::now();
    let mut buffer = [0u8; NETCODE_MAX_PACKET_BYTES];
    let mut usernames: HashMap<u64, String> = HashMap::new();
    loop {
        server.update(Instant::now() - last_updated);
        received_messages.clear();

        loop {
            match udp_socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    // println!("Received decrypted message {:?} from {}.", &buffer[..len], addr);
                    let server_result = server.process_packet(addr, &mut buffer[..len]);
                    handle_server_result(server_result, &udp_socket, &mut received_messages, &mut usernames);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("Socket error: {}", e),
            };
        }

        for text in received_messages.iter() {
            for client_id in server.clients_id().iter() {
                let (addr, payload) = server.generate_payload_packet(*client_id, text.as_bytes()).unwrap();
                udp_socket.send_to(payload, addr).unwrap();
            }
        }

        for client_id in server.clients_id().into_iter() {
            let server_result = server.update_client(client_id);
            handle_server_result(server_result, &udp_socket, &mut received_messages, &mut usernames);
        }

        last_updated = Instant::now();
        thread::sleep(Duration::from_millis(50));
    }
}

fn client(authentication: ClientAuthentication) {
    let udp_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp_socket.set_nonblocking(true).unwrap();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let mut client = NetcodeClient::new(now, authentication).unwrap();
    let stdin_channel = spawn_stdin_channel();
    let mut buffer = [0u8; NETCODE_MAX_PACKET_BYTES];

    let mut last_updated = Instant::now();
    loop {
        if let Some(err) = client.disconnect_reason() {
            panic!("Client error: {:?}", err);
        }

        match stdin_channel.try_recv() {
            Ok(text) => {
                if client.is_connected() {
                    let (addr, payload) = client.generate_payload_packet(text.as_bytes()).unwrap();
                    udp_socket.send_to(payload, addr).unwrap();
                } else {
                    println!("Client is not yet connected");
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("Stdin channel disconnected"),
        }

        loop {
            match udp_socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    if addr != client.server_addr() {
                        // Ignore packets that are not from the server
                        continue;
                    }
                    // println!("Received decrypted message {:?} from server {}", &buffer[..len], addr);
                    if let Some(payload) = client.process_packet(&mut buffer[..len]) {
                        let text = String::from_utf8(payload.to_vec()).unwrap();
                        println!("Received message from server: {}", text);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("Socket error: {}", e),
            };
        }

        if let Some((packet, addr)) = client.update(Instant::now() - last_updated) {
            udp_socket.send_to(packet, addr).unwrap();
        }
        last_updated = Instant::now();
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
