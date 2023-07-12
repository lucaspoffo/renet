use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::{shape::Icosphere, *},
};
use bevy_egui::EguiPlugin;
use bevy_renet::{
    renet::RenetClient, renet_steam_transport::transport::client::SteamClientTransport, steam_transport::SteamClientPlugin,
    RenetClientPlugin,
};
use demo_bevy::{client::*, connection_config, setup_level, NetworkedEntities, PlayerCommand, PlayerInput, ServerChannel, ServerMessages};
use renet_visualizer::{RenetClientVisualizer, RenetVisualizerStyle};
use smooth_bevy_cameras::LookTransformPlugin;
use steamworks::{Client, ClientManager, SingleClient, SteamId};

const YOUR_STEAM_ID: u64 = 123456789;

#[derive(Resource, Clone)]
struct SteamClient(Client<ClientManager>);

fn new_steam_client(steam_client: &Client<ClientManager>) -> (RenetClient, SteamClientTransport) {
    let client = RenetClient::new(connection_config());

    let transport = SteamClientTransport::new(&steam_client, &SteamId::from_raw(YOUR_STEAM_ID)).unwrap();

    (client, transport)
}

fn main() {
    let mut app = App::new();
    let (steam_client, single) = match Client::init() {
        Ok(result) => result,
        Err(err) => panic!("Failed to initialize Steam client: {}", err),
    };
    steam_client.networking_utils().init_relay_network_access();
    for _ in 0..5 {
        single.run_callbacks();
        std::thread::sleep(::std::time::Duration::from_millis(50));
    }
    app.add_plugins(DefaultPlugins);
    app.add_plugin(RenetClientPlugin);
    app.add_plugin(SteamClientPlugin);
    app.add_plugin(LookTransformPlugin);
    app.add_plugin(FrameTimeDiagnosticsPlugin::default());
    app.add_plugin(LogDiagnosticsPlugin::default());
    app.add_plugin(EguiPlugin);

    app.add_event::<PlayerCommand>();

    app.insert_resource(ClientLobby::default());
    app.insert_resource(PlayerInput::default());
    let (client, transport) = new_steam_client(&steam_client);

    app.insert_resource(client);
    app.insert_resource(transport);

    app.insert_resource(SteamClient(steam_client));
    app.insert_non_send_resource(single);
    app.add_system(steam_callbacks.in_base_set(CoreSet::First));

    app.insert_resource(NetworkMapping::default());

    app.add_systems((player_input, camera_follow, update_target_system));
    app.add_systems(
        (client_send_input, client_send_player_commands, client_sync_players)
            .distributive_run_if(bevy_renet::steam_transport::client_connected),
    );

    app.insert_resource(RenetClientVisualizer::<200>::new(RenetVisualizerStyle::default()));
    app.add_system(update_visulizer_system);

    app.add_startup_systems((setup_level, setup_camera, setup_target));

    app.run();
}

fn client_sync_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut client: ResMut<RenetClient>,
    steam_client: Res<SteamClient>,
    mut lobby: ResMut<ClientLobby>,
    mut network_mapping: ResMut<NetworkMapping>,
) {
    let client_id = steam_client.0.user().steam_id().raw();
    while let Some(message) = client.receive_message(ServerChannel::ServerMessages) {
        let server_message = bincode::deserialize(&message).unwrap();
        match server_message {
            ServerMessages::PlayerCreate { id, translation, entity } => {
                println!("Player {} connected.", id);
                let mut client_entity = commands.spawn(PbrBundle {
                    mesh: meshes.add(Mesh::from(shape::Capsule::default())),
                    material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                    transform: Transform::from_xyz(translation[0], translation[1], translation[2]),
                    ..Default::default()
                });

                if client_id == id {
                    client_entity.insert(ControlledPlayer);
                }

                let player_info = PlayerInfo {
                    server_entity: entity,
                    client_entity: client_entity.id(),
                };
                lobby.players.insert(id, player_info);
                network_mapping.0.insert(entity, client_entity.id());
            }
            ServerMessages::PlayerRemove { id } => {
                println!("Player {} disconnected.", id);
                if let Some(PlayerInfo {
                    server_entity,
                    client_entity,
                }) = lobby.players.remove(&id)
                {
                    commands.entity(client_entity).despawn();
                    network_mapping.0.remove(&server_entity);
                }
            }
            ServerMessages::SpawnProjectile { entity, translation } => {
                let projectile_entity = commands.spawn(PbrBundle {
                    mesh: meshes.add(
                        Mesh::try_from(Icosphere {
                            radius: 0.1,
                            subdivisions: 5,
                        })
                        .unwrap(),
                    ),
                    material: materials.add(Color::rgb(1.0, 0.0, 0.0).into()),
                    transform: Transform::from_translation(translation.into()),
                    ..Default::default()
                });
                network_mapping.0.insert(entity, projectile_entity.id());
            }
            ServerMessages::DespawnProjectile { entity } => {
                if let Some(entity) = network_mapping.0.remove(&entity) {
                    commands.entity(entity).despawn();
                }
            }
        }
    }

    while let Some(message) = client.receive_message(ServerChannel::NetworkedEntities) {
        let networked_entities: NetworkedEntities = bincode::deserialize(&message).unwrap();

        for i in 0..networked_entities.entities.len() {
            if let Some(entity) = network_mapping.0.get(&networked_entities.entities[i]) {
                let translation = networked_entities.translations[i].into();
                let transform = Transform {
                    translation,
                    ..Default::default()
                };
                commands.entity(*entity).insert(transform);
            }
        }
    }
}

fn steam_callbacks(client: NonSend<SingleClient>) {
    client.run_callbacks();
}
