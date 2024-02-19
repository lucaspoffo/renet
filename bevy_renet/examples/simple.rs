use bevy::prelude::{shape::Plane, *};
use bevy_renet::{
    client_connected,
    renet::{
        transport::{ClientAuthentication, ServerAuthentication, ServerConfig},
        ConnectionConfig, DefaultChannel, RenetClient, RenetServer, ServerEvent,
    },
    transport::{NetcodeClientPlugin, NetcodeServerPlugin},
    RenetClientPlugin, RenetServerPlugin,
};
use renet::{
    transport::{NetcodeClientTransport, NetcodeServerTransport, NetcodeTransportError},
    ClientId,
};

use std::time::SystemTime;
use std::{collections::HashMap, net::UdpSocket};

use serde::{Deserialize, Serialize};

const PROTOCOL_ID: u64 = 7;

const PLAYER_MOVE_SPEED: f32 = 1.0;

#[derive(Debug, Default, Serialize, Deserialize, Component, Resource)]
struct PlayerInput {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

#[derive(Debug, Component)]
struct Player {
    id: ClientId,
}

#[derive(Debug, Default, Resource)]
struct Lobby {
    players: HashMap<ClientId, Entity>,
}

#[derive(Debug, Serialize, Deserialize, Component)]
enum ServerMessages {
    PlayerConnected { id: ClientId },
    PlayerDisconnected { id: ClientId },
}

fn new_renet_client() -> (RenetClient, NetcodeClientTransport) {
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let client_id = current_time.as_millis() as u64;
    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        server_addr,
        user_data: None,
    };

    let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();
    let client = RenetClient::new(ConnectionConfig::default());

    (client, transport)
}

fn new_renet_server() -> (RenetServer, NetcodeServerTransport) {
    let public_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(public_addr).unwrap();
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let server_config = ServerConfig {
        current_time,
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        public_addresses: vec![public_addr],
        authentication: ServerAuthentication::Unsecure,
    };

    let transport = NetcodeServerTransport::new(server_config, socket).unwrap();
    let server = RenetServer::new(ConnectionConfig::default());

    (server, transport)
}

fn main() {
    println!("Usage: run with \"server\" or \"client\" argument");
    let args: Vec<String> = std::env::args().collect();

    let exec_type = &args[1];
    let is_host = match exec_type.as_str() {
        "client" => false,
        "server" => true,
        _ => panic!("Invalid argument, must be \"client\" or \"server\"."),
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.init_resource::<Lobby>();

    if is_host {
        app.add_plugins(RenetServerPlugin);
        app.add_plugins(NetcodeServerPlugin);
        let (server, transport) = new_renet_server();
        app.insert_resource(server);
        app.insert_resource(transport);

        app.add_systems(
            Update,
            (server_update_system, server_sync_players, move_players_system).run_if(resource_exists::<RenetServer>),
        );
    } else {
        app.add_plugins(RenetClientPlugin);
        app.add_plugins(NetcodeClientPlugin);
        app.init_resource::<PlayerInput>();
        let (client, transport) = new_renet_client();
        app.insert_resource(client);
        app.insert_resource(transport);

        app.add_systems(
            Update,
            (player_input, client_send_input, client_sync_players).run_if(client_connected()),
        );
    }

    app.add_systems(Startup, setup);
    app.add_systems(Update, panic_on_error_system);

    app.run();
}

fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<Lobby>,
    mut server: ResMut<RenetServer>,
) {
    for event in server_events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Player {} connected.", client_id);
                // Spawn player cube
                let player_entity = commands
                    .spawn(PbrBundle {
                        mesh: meshes.add(Cuboid::from_size(Vec3::splat(1.0))),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6)),
                        transform: Transform::from_xyz(0.0, 0.5, 0.0),
                        ..Default::default()
                    })
                    .insert(PlayerInput::default())
                    .insert(Player { id: *client_id })
                    .id();

                // We could send an InitState with all the players id and positions for the client
                // but this is easier to do.
                for &player_id in lobby.players.keys() {
                    let message = bincode::serialize(&ServerMessages::PlayerConnected { id: player_id }).unwrap();
                    server.send_message(*client_id, DefaultChannel::ReliableOrdered, message);
                }

                lobby.players.insert(*client_id, player_entity);

                let message = bincode::serialize(&ServerMessages::PlayerConnected { id: *client_id }).unwrap();
                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Player {} disconnected: {}", client_id, reason);
                if let Some(player_entity) = lobby.players.remove(client_id) {
                    commands.entity(player_entity).despawn();
                }

                let message = bincode::serialize(&ServerMessages::PlayerDisconnected { id: *client_id }).unwrap();
                server.broadcast_message(DefaultChannel::ReliableOrdered, message);
            }
        }
    }

    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered) {
            let player_input: PlayerInput = bincode::deserialize(&message).unwrap();
            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(player_input);
            }
        }
    }
}

fn server_sync_players(mut server: ResMut<RenetServer>, query: Query<(&Transform, &Player)>) {
    let mut players: HashMap<ClientId, [f32; 3]> = HashMap::new();
    for (transform, player) in query.iter() {
        players.insert(player.id, transform.translation.into());
    }

    let sync_message = bincode::serialize(&players).unwrap();
    server.broadcast_message(DefaultChannel::Unreliable, sync_message);
}

fn client_sync_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut client: ResMut<RenetClient>,
    mut lobby: ResMut<Lobby>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        let server_message = bincode::deserialize(&message).unwrap();
        match server_message {
            ServerMessages::PlayerConnected { id } => {
                println!("Player {} connected.", id);
                let player_entity = commands
                    .spawn(PbrBundle {
                        mesh: meshes.add(Cuboid::from_size(Vec3::splat(1.0))),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6)),
                        transform: Transform::from_xyz(0.0, 0.5, 0.0),
                        ..Default::default()
                    })
                    .id();

                lobby.players.insert(id, player_entity);
            }
            ServerMessages::PlayerDisconnected { id } => {
                println!("Player {} disconnected.", id);
                if let Some(player_entity) = lobby.players.remove(&id) {
                    commands.entity(player_entity).despawn();
                }
            }
        }
    }

    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let players: HashMap<ClientId, [f32; 3]> = bincode::deserialize(&message).unwrap();
        for (player_id, translation) in players.iter() {
            if let Some(player_entity) = lobby.players.get(player_id) {
                let transform = Transform {
                    translation: (*translation).into(),
                    ..Default::default()
                };
                commands.entity(*player_entity).insert(transform);
            }
        }
    }
}

/// set up a simple 3D scene
fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // plane
    commands.spawn(PbrBundle {
        mesh: meshes.add(Mesh::from(Plane::from_size(5.0))),
        material: materials.add(Color::rgb(0.3, 0.5, 0.3)),
        ..Default::default()
    });
    // light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            intensity: 1500.0,
            shadows_enabled: true,
            ..Default::default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..Default::default()
    });
    // camera
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..Default::default()
    });
}

fn player_input(keyboard_input: Res<ButtonInput<KeyCode>>, mut player_input: ResMut<PlayerInput>) {
    player_input.left = keyboard_input.pressed(KeyCode::KeyA) || keyboard_input.pressed(KeyCode::ArrowLeft);
    player_input.right = keyboard_input.pressed(KeyCode::KeyD) || keyboard_input.pressed(KeyCode::ArrowRight);
    player_input.up = keyboard_input.pressed(KeyCode::KeyW) || keyboard_input.pressed(KeyCode::ArrowUp);
    player_input.down = keyboard_input.pressed(KeyCode::KeyS) || keyboard_input.pressed(KeyCode::ArrowDown);
}

fn client_send_input(player_input: Res<PlayerInput>, mut client: ResMut<RenetClient>) {
    let input_message = bincode::serialize(&*player_input).unwrap();

    client.send_message(DefaultChannel::ReliableOrdered, input_message);
}

fn move_players_system(mut query: Query<(&mut Transform, &PlayerInput)>, time: Res<Time>) {
    for (mut transform, input) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.down as i8 - input.up as i8) as f32;
        transform.translation.x += x * PLAYER_MOVE_SPEED * time.delta().as_secs_f32();
        transform.translation.z += y * PLAYER_MOVE_SPEED * time.delta().as_secs_f32();
    }
}

// If any error is found we just panic
#[allow(clippy::never_loop)]
fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
    for e in renet_error.read() {
        panic!("{}", e);
    }
}
