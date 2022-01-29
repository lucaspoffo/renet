use renetcode::{
    client::Client,
    server::{Server, ServerEvent, ServerResult},
    token::ConnectToken,
    NETCODE_KEY_BYTES, NETCODE_MAX_PACKET_BYTES,
};
use std::thread;
use std::time::Duration;
use std::{
    net::{SocketAddr, UdpSocket},
    time::Instant,
};
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{SystemTime, UNIX_EPOCH},
};

const PROTOCOL_ID: u64 = 123456789;

fn main() {
    println!("Usage: server [SERVER_PORT] or client [SERVER_PORT]");
    let args: Vec<String> = std::env::args().collect();
    let private_key = b"an example very very secret key."; // 32-bytes

    let exec_type = &args[1];
    match exec_type.as_str() {
        "client" => {
            let server_addr: SocketAddr = format!("127.0.0.1:{}", args[2]).parse().unwrap();
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let client_id = now.as_millis() as u64;
            // TODO: add username as arg and pass to user data
            let connect_token =
                ConnectToken::generate(now, PROTOCOL_ID, 300, client_id, 15, vec![server_addr], None, &private_key).unwrap();
            client(connect_token);
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

fn server(addr: SocketAddr, private_key: [u8; NETCODE_KEY_BYTES]) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let mut server: Server = Server::new(now, 16, PROTOCOL_ID, addr, private_key);
    let udp_socket = UdpSocket::bind(addr).unwrap();
    udp_socket.set_nonblocking(true).unwrap();
    let mut received_messages = vec![];
    let mut last_updated = Instant::now();
    let mut buffer = [0u8; NETCODE_MAX_PACKET_BYTES];
    loop {
        server.advance_time(Instant::now() - last_updated);
        received_messages.clear();
        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected(id) => println!("Client {} connected.", id),
                ServerEvent::ClientDisconnected(id) => println!("Client {} disconnected.", id),
            }
        }

        loop {
            match udp_socket.recv_from(&mut buffer) {
                Ok((len, addr)) => {
                    // println!("Received decrypted message {:?} from {}.", &buffer[..len], addr);
                    match server.process_packet(addr, &mut buffer[..len]) {
                        ServerResult::Payload(client_id, payload) => {
                            let text = String::from_utf8(payload.to_vec()).unwrap();
                            println!("Client {} sent message {:?}.", client_id, text);
                            received_messages.push(text);
                        }
                        ServerResult::PacketToSend(packet) => {
                            udp_socket.send_to(packet, addr).unwrap();
                        }
                        ServerResult::None => {}
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("Socket error: {}", e),
            };
        }

        for text in received_messages.iter() {
            for client_id in server.clients_id().iter() {
                let (payload, addr) = server.generate_payload_packet(*client_id, text.as_bytes()).unwrap();
                udp_socket.send_to(payload, addr).unwrap();
            }
        }

        for i in 0..server.max_clients() {
            if let Some((packet, addr)) = server.update_client(i) {
                udp_socket.send_to(packet, addr).unwrap();
            }
        }

        server.update_pending_connections();
        last_updated = Instant::now();
        thread::sleep(Duration::from_millis(50));
    }
}

fn client(connect_token: ConnectToken) {
    let udp_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp_socket.set_nonblocking(true).unwrap();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let mut client = Client::new(now, connect_token);
    let stdin_channel = spawn_stdin_channel();
    let mut buffer = [0u8; NETCODE_MAX_PACKET_BYTES];

    let mut last_updated = Instant::now();
    loop {
        client.update(Instant::now() - last_updated).unwrap();
        last_updated = Instant::now();
        if let Some(err) = client.error() {
            panic!("Client error: {:?}", err);
        }

        match stdin_channel.try_recv() {
            Ok(text) => {
                if client.connected() {
                    let (payload, addr) = client.generate_payload_packet(text.as_bytes()).unwrap();
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

        if let Some((packet, addr)) = client.generate_packet() {
            udp_socket.send_to(packet, addr).unwrap();
        }
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
