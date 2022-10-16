use std::{collections::HashMap, net::UdpSocket, time::SystemTime};

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};
use bevy_egui::{EguiContext, EguiPlugin};
use bevy_rapier3d::prelude::*;
use bevy_renet::{
    renet::{RenetServer, ServerAuthentication, ServerConfig, ServerEvent, UdpTransport},
    RenetServerPlugin,
};
use demo_bevy::{
    server_connection_config, setup_level, spawn_fireball, ClientChannel, NetworkFrame, Player, PlayerCommand, PlayerInput, Projectile,
    ServerChannel, ServerMessages, PROTOCOL_ID,
};
use renet_visualizer::RenetServerVisualizer;

#[derive(Debug, Default)]
pub struct ServerLobby {
    pub players: HashMap<u64, Entity>,
}

#[derive(Debug, Default)]
struct NetworkTick(u32);

// Clients last received ticks
#[derive(Debug, Default)]
struct ClientTicks(HashMap<u64, Option<u32>>);

const PLAYER_MOVE_SPEED: f32 = 5.0;

fn new_renet_server() -> RenetServer {
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(server_addr).unwrap();
    let transport = Box::new(UdpTransport::with_socket(socket).unwrap()) as _;
    let connection_config = server_connection_config();
    let server_config = ServerConfig::new(64, PROTOCOL_ID, server_addr, ServerAuthentication::Unsecure);
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    RenetServer::new(current_time, server_config, connection_config, transport)
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);

    app.add_plugin(RenetServerPlugin::default());
    app.add_plugin(RapierPhysicsPlugin::<NoUserData>::default());
    app.add_plugin(RapierDebugRenderPlugin::default());
    app.add_plugin(FrameTimeDiagnosticsPlugin::default());
    app.add_plugin(LogDiagnosticsPlugin::default());
    app.add_plugin(EguiPlugin);

    app.insert_resource(ServerLobby::default());
    app.insert_resource(NetworkTick(0));
    app.insert_resource(ClientTicks::default());
    app.insert_resource(new_renet_server());
    app.insert_resource(RenetServerVisualizer::<200>::default());

    app.add_system(server_update_system);
    app.add_system(server_network_sync);
    app.add_system(move_players_system);
    app.add_system(update_projectiles_system);
    app.add_system(update_visulizer_system);
    app.add_system(despawn_projectile_system);
    app.add_system_to_stage(CoreStage::PostUpdate, projectile_on_removal_system);

    app.add_startup_system(setup_level);
    app.add_startup_system(setup_simple_camera);

    app.run();
}

#[allow(clippy::too_many_arguments)]
fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    mut visualizer: ResMut<RenetServerVisualizer<200>>,
    mut client_ticks: ResMut<ClientTicks>,
    players: Query<(Entity, &Player, &Transform)>,
) {
    for event in server_events.iter() {
        match event {
            ServerEvent::ClientConnected(id, _) => {
                println!("Player {} connected.", id);
                visualizer.add_client(*id);

                // Initialize other players for this new client
                for (entity, player, transform) in players.iter() {
                    let translation: [f32; 3] = transform.translation.into();
                    let message = bincode::serialize(&ServerMessages::PlayerCreate {
                        id: player.id,
                        entity,
                        translation,
                    })
                    .unwrap();
                    server.send_message(*id, ServerChannel::ServerMessages.id(), message);
                }

                // Spawn new player
                let transform = Transform::from_xyz(0.0, 0.51, 0.0);
                let player_entity = commands
                    .spawn_bundle(PbrBundle {
                        mesh: meshes.add(Mesh::from(shape::Capsule::default())),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                        transform,
                        ..Default::default()
                    })
                    .insert(RigidBody::Dynamic)
                    .insert(LockedAxes::ROTATION_LOCKED | LockedAxes::TRANSLATION_LOCKED_Y)
                    .insert(Collider::capsule_y(0.5, 0.5))
                    .insert(PlayerInput::default())
                    .insert(Velocity::default())
                    .insert(Player { id: *id })
                    .id();

                lobby.players.insert(*id, player_entity);

                let translation: [f32; 3] = transform.translation.into();
                let message = bincode::serialize(&ServerMessages::PlayerCreate {
                    id: *id,
                    entity: player_entity,
                    translation,
                })
                .unwrap();
                server.broadcast_message(ServerChannel::ServerMessages.id(), message);
            }
            ServerEvent::ClientDisconnected(id) => {
                println!("Player {} disconnected.", id);
                visualizer.remove_client(*id);
                client_ticks.0.remove(id);
                if let Some(player_entity) = lobby.players.remove(id) {
                    commands.entity(player_entity).despawn();
                }

                let message = bincode::serialize(&ServerMessages::PlayerRemove { id: *id }).unwrap();
                server.broadcast_message(ServerChannel::ServerMessages.id(), message);
            }
        }
    }

    for client_id in server.clients_id().into_iter() {
        while let Some(message) = server.receive_message(client_id, ClientChannel::Command.id()) {
            let command: PlayerCommand = bincode::deserialize(&message).unwrap();
            match command {
                PlayerCommand::BasicAttack { mut cast_at } => {
                    println!("Received basic attack from client {}: {:?}", client_id, cast_at);

                    if let Some(player_entity) = lobby.players.get(&client_id) {
                        if let Ok((_, _, player_transform)) = players.get(*player_entity) {
                            cast_at[1] = player_transform.translation[1];

                            let direction = (cast_at - player_transform.translation).normalize_or_zero();
                            let mut translation = player_transform.translation + (direction * 0.7);
                            translation[1] = 1.0;

                            let fireball_entity = spawn_fireball(&mut commands, &mut meshes, &mut materials, translation, direction);
                            let message = ServerMessages::SpawnProjectile {
                                entity: fireball_entity,
                                translation: translation.into(),
                            };
                            let message = bincode::serialize(&message).unwrap();
                            server.broadcast_message(ServerChannel::ServerMessages.id(), message);
                        }
                    }
                }
            }
        }
        while let Some(message) = server.receive_message(client_id, ClientChannel::Input.id()) {
            let input: PlayerInput = bincode::deserialize(&message).unwrap();
            client_ticks.0.insert(client_id, input.most_recent_tick);
            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(input);
            }
        }
    }
}

fn update_projectiles_system(mut commands: Commands, mut projectiles: Query<(Entity, &mut Projectile)>, time: Res<Time>) {
    for (entity, mut projectile) in projectiles.iter_mut() {
        projectile.duration.tick(time.delta());
        if projectile.duration.finished() {
            commands.entity(entity).despawn();
        }
    }
}

fn update_visulizer_system(
    mut egui_context: ResMut<EguiContext>,
    mut visualizer: ResMut<RenetServerVisualizer<200>>,
    server: Res<RenetServer>,
) {
    visualizer.update(&server);
    visualizer.show_window(egui_context.ctx_mut());
}

#[allow(clippy::type_complexity)]
fn server_network_sync(
    mut tick: ResMut<NetworkTick>,
    mut server: ResMut<RenetServer>,
    networked_entities: Query<(Entity, &Transform), Or<(With<Player>, With<Projectile>)>>,
) {
    let mut frame = NetworkFrame::default();
    for (entity, transform) in networked_entities.iter() {
        frame.entities.entities.push(entity);
        frame.entities.translations.push(transform.translation.into());
    }

    frame.tick = tick.0;
    tick.0 += 1;
    let sync_message = bincode::serialize(&frame).unwrap();
    server.broadcast_message(ServerChannel::NetworkFrame.id(), sync_message);
}

fn move_players_system(mut query: Query<(&mut Velocity, &PlayerInput)>) {
    for (mut velocity, input) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.down as i8 - input.up as i8) as f32;
        let direction = Vec2::new(x, y).normalize_or_zero();
        velocity.linvel.x = direction.x * PLAYER_MOVE_SPEED;
        velocity.linvel.z = direction.y * PLAYER_MOVE_SPEED;
    }
}

pub fn setup_simple_camera(mut commands: Commands) {
    // camera
    commands.spawn_bundle(Camera3dBundle {
        transform: Transform::from_xyz(-5.5, 5.0, 5.5).looking_at(Vec3::ZERO, Vec3::Y),
        ..Default::default()
    });
}

fn despawn_projectile_system(
    mut commands: Commands,
    mut collision_events: EventReader<CollisionEvent>,
    projectile_query: Query<Option<&Projectile>>,
) {
    for collision_event in collision_events.iter() {
        if let CollisionEvent::Started(entity1, entity2, _) = collision_event {
            if let Ok(Some(_)) = projectile_query.get(*entity1) {
                commands.entity(*entity1).despawn();
            }
            if let Ok(Some(_)) = projectile_query.get(*entity2) {
                commands.entity(*entity2).despawn();
            }
        }
    }
}

fn projectile_on_removal_system(mut server: ResMut<RenetServer>, removed_projectiles: RemovedComponents<Projectile>) {
    for entity in removed_projectiles.iter() {
        let message = ServerMessages::DespawnProjectile { entity };
        let message = bincode::serialize(&message).unwrap();

        server.broadcast_message(ServerChannel::ServerMessages.id(), message);
    }
}
