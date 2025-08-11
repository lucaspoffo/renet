use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use renet::{ConnectionConfig, DefaultChannel, RenetClient, RenetServer, ServerEvent};
use renet_steam::{AccessPermission, SteamClientTransport, SteamServerConfig, SteamServerTransport};
use steamworks::{Client, LobbyId, LobbyType, SteamId};

fn main() {
    env_logger::init();
    let steam_client = Client::init_app(480).unwrap();
    steam_client.networking_utils().init_relay_network_access();

    println!("Usage:");
    println!("\tclient [SERVER_STEAM_ID] [LOBBY_ID?]");
    println!("\tserver [lobby?]");

    let args: Vec<String> = std::env::args().collect();

    let exec_type = &args[1];
    match exec_type.as_str() {
        "client" => {
            let server_steam_id: u64 = args[2].parse().unwrap();
            let mut lobby_id: Option<LobbyId> = None;
            if let Some(lobby) = args.get(3) {
                let id: u64 = lobby.parse().unwrap();
                lobby_id = Some(LobbyId::from_raw(id));
            }
            run_client(steam_client, SteamId::from_raw(server_steam_id), lobby_id);
        }
        "server" => {
            let mut with_lobby = false;
            if let Some(lobby) = args.get(2) {
                with_lobby = lobby.as_str() == "lobby";
            }
            run_server(steam_client, with_lobby);
        }
        _ => {
            println!("Invalid argument, first one must be \"client\" or \"server\".");
        }
    }
}

fn run_server(steam_client: Client, with_lobby: bool) {
    // Create lobby if necessary
    let access_permission = if with_lobby {
        let (sender_create_lobby, receiver_create_lobby) = mpsc::channel();
        steam_client.matchmaking().create_lobby(LobbyType::Public, 10, move |lobby| {
            match lobby {
                Ok(lobby) => {
                    sender_create_lobby.send(lobby).unwrap();
                }
                Err(e) => panic!("Failed to create lobby: {e}"),
            };
        });

        loop {
            steam_client.run_callbacks();
            if let Ok(lobby) = receiver_create_lobby.try_recv() {
                println!("Created lobby with id: {}", lobby.raw());
                break AccessPermission::InLobby(lobby);
            }
            thread::sleep(Duration::from_millis(20));
        }
    } else {
        AccessPermission::Public
    };

    let connection_config = ConnectionConfig::default();
    let mut server: RenetServer = RenetServer::new(connection_config);
    let steam_transport_config = SteamServerConfig {
        max_clients: 10,
        access_permission,
    };
    let mut transport = SteamServerTransport::new(&steam_client, steam_transport_config).unwrap();

    let mut received_messages = vec![];
    let mut last_updated = Instant::now();

    loop {
        steam_client.run_callbacks();
        let now = Instant::now();
        let duration = now - last_updated;
        last_updated = now;

        server.update(duration);
        transport.update(&mut server);

        received_messages.clear();

        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    println!("Client {} connected.", client_id)
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    println!("Client {} disconnected: {}", client_id, reason);
                }
            }
        }

        for client_id in server.clients_id() {
            while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered) {
                let text = String::from_utf8(message.into()).unwrap();
                println!("Client {} sent text: {}", client_id, text);
                let text = format!("{}: {}", client_id, text);
                received_messages.push(text);
            }
        }

        for text in received_messages.iter() {
            server.broadcast_message(DefaultChannel::ReliableOrdered, text.as_bytes().to_vec());
        }

        transport.send_packets(&mut server);
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_client(steam_client: Client, server_steam_id: SteamId, lobby_id: Option<LobbyId>) {
    // Connect to lobby
    if let Some(lobby_id) = lobby_id {
        let (sender_join_lobby, receiver_join_lobby) = mpsc::channel();
        steam_client.matchmaking().join_lobby(lobby_id, move |lobby| {
            match lobby {
                Ok(lobby) => {
                    sender_join_lobby.send(lobby).unwrap();
                }
                Err(e) => panic!("Failed to join lobby: {e:?}"),
            };
        });

        loop {
            steam_client.run_callbacks();
            if let Ok(lobby) = receiver_join_lobby.try_recv() {
                println!("Joined lobby with id: {}", lobby.raw());
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    let connection_config = ConnectionConfig::default();
    let mut client = RenetClient::new(connection_config);

    let mut transport = SteamClientTransport::new(&steam_client, &server_steam_id).unwrap();
    let stdin_channel: Receiver<String> = spawn_stdin_channel();

    let mut last_updated = Instant::now();
    loop {
        steam_client.run_callbacks();
        let now = Instant::now();
        let duration = now - last_updated;
        last_updated = now;

        client.update(duration);
        transport.update(&mut client);

        if client.is_connected() {
            match stdin_channel.try_recv() {
                Ok(text) => {
                    client.send_message(DefaultChannel::ReliableOrdered, text.as_bytes().to_vec());
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => panic!("Channel disconnected"),
            }

            while let Some(text) = client.receive_message(DefaultChannel::ReliableOrdered) {
                let text = String::from_utf8(text.into()).unwrap();
                println!("{}", text);
            }
        }

        transport.send_packets(&mut client).unwrap();
        thread::sleep(Duration::from_millis(20));
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
