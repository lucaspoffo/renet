use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant, SystemTime},
};

use renet::{ClientAuthentication, ConnectToken, DefaultChannel, RenetClient, RenetConnectionConfig};
use renet_transport_steam::SteamTransport;
use steamworks::{Client, LobbyId};
use demo_steam::CONNECT_TOKEN_CHANNEL;

fn main() {
    println!("Usage: cargo run --bin client -- [LOBBY_ID]");
    let args: Vec<String> = std::env::args().collect();

    let lobby_id = args[1].parse::<u64>().unwrap();
    client(LobbyId::from_raw(lobby_id));
}

enum ClientState {
    WaitingLobby,
    WaitingConnectToken { lobby_id: LobbyId },
    Client { client: RenetClient },
}

fn client(lobby_id: LobbyId) {
    let (client, single_client) = Client::init().unwrap();

    let mut state = ClientState::WaitingLobby;

    let matchmaking = client.matchmaking();
    let networking_messages = client.networking_messages();

    let (sender_join_lobby, receiver_join_lobby) = mpsc::channel();

    matchmaking.join_lobby(lobby_id, move |result| match result {
        Ok(lobby) => sender_join_lobby.send(lobby).unwrap(),
        Err(_) => panic!("Failed to connect lobby"),
    });

    let stdin_channel = spawn_stdin_channel();

    let mut last_updated = Instant::now();
    loop {
        single_client.run_callbacks();
        let now = Instant::now();
        let delta = now - last_updated;
        last_updated = now;
        match state {
            ClientState::WaitingLobby => {
                if let Ok(lobby_id) = receiver_join_lobby.recv() {
                    state = ClientState::WaitingConnectToken { lobby_id };
                }
            }
            ClientState::WaitingConnectToken { lobby_id } => {
                for message in networking_messages.receive_messages_on_channel(CONNECT_TOKEN_CHANNEL, 1).iter() {
                    let data = message.data().to_vec();
                    if let Ok(connect_token) = ConnectToken::read(&mut data.as_slice()) {
                        let connection_config = RenetConnectionConfig::default();

                        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
                        let authentication = ClientAuthentication::Secure { connect_token };

                        let mut transport = SteamTransport::new(&client);

                        let client_clone = client.clone();
                        let lobby_id_clone = lobby_id.clone();
                        // Only accepts request from the lobby owner.
                        transport.session_request_callback(move |request| {
                            let matchmaking = client_clone.matchmaking();
                            let lobby_owner = matchmaking.lobby_owner(lobby_id_clone);
                            if let Some(remote_user) = request.remote().steam_id() {
                                if remote_user == lobby_owner {
                                    request.accept();
                                    return;
                                }
                            }

                            request.reject();
                        });

                        let client = RenetClient::new(current_time, connection_config, authentication, Box::new(transport)).unwrap();
                        state = ClientState::Client { client };
                    }
                }
            }
            ClientState::Client { ref mut client } => {
                client.update(delta).unwrap();
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
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn spawn_stdin_channel() -> Receiver<String> {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || loop {
        let mut buffer = String::new();
        std::io::stdin().read_line(&mut buffer).unwrap();
        tx.send(buffer.trim_end().to_string()).unwrap();
    });
    rx
}
