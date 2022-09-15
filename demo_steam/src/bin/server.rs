use std::{
    sync::mpsc,
    time::{Duration, Instant, SystemTime},
};

use renet::{
    generate_random_bytes, ConnectToken, DefaultChannel, RenetConnectionConfig, RenetServer, ServerAuthentication, ServerConfig,
    ServerEvent,
};
use renet_transport_steam::{address_from_steam_id, SteamTransport};
use steamworks::{networking_types::SendFlags, Client, LobbyType};
use demo_steam::{PROTOCOL_ID, CONNECT_TOKEN_CHANNEL};

fn main() {
    println!("Usage: cargo run --bin server");

    server();
}

enum ServerState {
    WaitingLobby,
    Lobby { server: RenetServer },
}

fn server() {
    let (client, single_client) = Client::init().unwrap();
    let matchmaking = client.matchmaking();

    let (sender_create_lobby, receiver_create_lobby) = mpsc::channel();
    let mut state = ServerState::WaitingLobby;

    matchmaking.create_lobby(LobbyType::FriendsOnly, 4, move |lobby| match lobby {
        Ok(lobby) => {
            sender_create_lobby.send(lobby).unwrap();
        }
        Err(e) => {
            panic!("Failed to create lobby: {}", e);
        }
    });

    let mut last_updated = Instant::now();

    loop {
        single_client.run_callbacks();
        let now = Instant::now();
        let delta = now - last_updated;
        last_updated = now;

        match &mut state {
            ServerState::WaitingLobby => {
                if let Ok(lobby_id) = receiver_create_lobby.try_recv() {
                    println!("Created lobby: {:?}", lobby_id);
                    let private_key = generate_random_bytes();

                    let client_clone = client.clone();
                    let mut transport = SteamTransport::new(&client);
                    transport.session_request_callback(move |request| {
                        let matchmaking = client_clone.matchmaking();
                        let networking_messages = client_clone.networking_messages();

                        let lobby_members = matchmaking.lobby_members(lobby_id);
                        let network_identity = request.remote().clone();
                        if let Some(remote_user) = request.remote().steam_id() {
                            if lobby_members.contains(&remote_user) {
                                request.accept();

                                let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
                                let fake_server_addr = address_from_steam_id(client_clone.user().steam_id());
                                let token = ConnectToken::generate(
                                    current_time,
                                    PROTOCOL_ID,
                                    360,
                                    remote_user.raw(),
                                    30,
                                    vec![fake_server_addr],
                                    None,
                                    &private_key,
                                )
                                .unwrap();

                                let mut data = vec![];
                                token.write(&mut data).unwrap();

                                if let Err(e) = networking_messages.send_message_to_user(
                                    network_identity,
                                    SendFlags::RELIABLE,
                                    &data,
                                    CONNECT_TOKEN_CHANNEL,
                                ) {
                                    println!("Failed to send ConnectToken: {e}");
                                }
                                return;
                            }
                        }
                        request.reject();
                    });

                    let server_addr = address_from_steam_id(client.user().steam_id());
                    let connection_config = RenetConnectionConfig::default();
                    let server_config = ServerConfig::new(64, PROTOCOL_ID, server_addr, ServerAuthentication::Secure { private_key });
                    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();

                    let server = RenetServer::new(current_time, server_config, connection_config, Box::new(transport));

                    state = ServerState::Lobby { server };
                }
            }
            ServerState::Lobby { ref mut server } => {
                server.update(delta).unwrap();

                let mut received_messages = vec![];
                while let Some(event) = server.get_event() {
                    match event {
                        ServerEvent::ClientConnected(id, _) => {
                            println!("Client {} connected.", id)
                        }
                        ServerEvent::ClientDisconnected(id) => {
                            println!("Client {} disconnected", id);
                        }
                    }
                }

                for client_id in server.clients_id().into_iter() {
                    while let Some(message) = server.receive_message(client_id, DefaultChannel::Reliable) {
                        let text = String::from_utf8(message).unwrap();
                        println!("Client {} sent text: {}", client_id, text);
                        let text = format!("{}: {}", client_id, text);
                        received_messages.push(text);
                    }
                }

                for text in received_messages.iter() {
                    server.broadcast_message(DefaultChannel::Reliable, text.as_bytes().to_vec());
                }

                server.send_packets().unwrap();
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}
