use std::{f32::consts::PI, time::Duration};

use bevy::prelude::*;
use bevy_renet::renet::{ChannelConfig, ClientId, ConnectionConfig, SendType};
use serde::{Deserialize, Serialize};

#[cfg(feature = "transport")]
pub const PRIVATE_KEY: &[u8; bevy_renet::renet::transport::NETCODE_KEY_BYTES] = b"an example very very secret key."; // 32-bytes
#[cfg(feature = "transport")]
pub const PROTOCOL_ID: u64 = 7;

#[derive(Debug, Component)]
pub struct Player {
    pub id: ClientId,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Component, Resource)]
pub struct PlayerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Debug, Serialize, Deserialize, Event)]
pub enum PlayerCommand {
    BasicAttack { cast_at: Vec3 },
}
pub enum ClientChannel {
    Input,
    Command,
}
pub enum ServerChannel {
    ServerMessages,
    NetworkedEntities,
}

#[derive(Debug, Default, Component)]
pub struct Velocity(pub Vec3);

#[derive(Debug, Serialize, Deserialize, Component)]
pub enum ServerMessages {
    PlayerCreate {
        entity: Entity,
        id: ClientId,
        translation: [f32; 3],
    },
    PlayerRemove {
        id: ClientId,
    },
    SpawnProjectile {
        entity: Entity,
        translation: [f32; 3],
    },
    DespawnProjectile {
        entity: Entity,
    },
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkedEntities {
    pub entities: Vec<Entity>,
    pub translations: Vec<[f32; 3]>,
}

impl From<ClientChannel> for u8 {
    fn from(channel_id: ClientChannel) -> Self {
        match channel_id {
            ClientChannel::Command => 0,
            ClientChannel::Input => 1,
        }
    }
}

impl ClientChannel {
    pub fn channels_config() -> Vec<ChannelConfig> {
        vec![
            ChannelConfig {
                channel_id: Self::Input.into(),
                max_memory_usage_bytes: 5 * 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::ZERO,
                },
            },
            ChannelConfig {
                channel_id: Self::Command.into(),
                max_memory_usage_bytes: 5 * 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::ZERO,
                },
            },
        ]
    }
}

impl From<ServerChannel> for u8 {
    fn from(channel_id: ServerChannel) -> Self {
        match channel_id {
            ServerChannel::NetworkedEntities => 0,
            ServerChannel::ServerMessages => 1,
        }
    }
}

impl ServerChannel {
    pub fn channels_config() -> Vec<ChannelConfig> {
        vec![
            ChannelConfig {
                channel_id: Self::NetworkedEntities.into(),
                max_memory_usage_bytes: 10 * 1024 * 1024,
                send_type: SendType::Unreliable,
            },
            ChannelConfig {
                channel_id: Self::ServerMessages.into(),
                max_memory_usage_bytes: 10 * 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::from_millis(200),
                },
            },
        ]
    }
}

pub fn connection_config() -> ConnectionConfig {
    ConnectionConfig {
        available_bytes_per_tick: 1024 * 1024,
        client_channels_config: ClientChannel::channels_config(),
        server_channels_config: ServerChannel::channels_config(),
    }
}

/// set up a simple 3D scene
pub fn setup_level(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // plane
    commands.spawn(PbrBundle {
        mesh: meshes.add(Mesh::from(Cuboid::new(40., 1., 40.))),
        material: materials.add(Color::srgb(0.3, 0.5, 0.3)),
        transform: Transform::from_xyz(0.0, -1.0, 0.0),
        ..Default::default()
    });
    // light
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            shadows_enabled: true,
            ..default()
        },
        transform: Transform {
            translation: Vec3::new(0.0, 2.0, 0.0),
            rotation: Quat::from_rotation_x(-PI / 4.),
            ..default()
        },
        ..default()
    });
}

#[derive(Debug, Component)]
pub struct Projectile {
    pub duration: Timer,
}

pub fn spawn_fireball(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    translation: Vec3,
    mut direction: Vec3,
) -> Entity {
    if !direction.is_normalized() {
        direction = Vec3::X;
    }
    commands
        .spawn(PbrBundle {
            mesh: meshes.add(Sphere { radius: 0.1 }),
            material: materials.add(Color::srgb(1.0, 0.0, 0.0)),
            transform: Transform::from_translation(translation),
            ..Default::default()
        })
        .insert(Velocity(direction * 10.))
        .insert(Projectile {
            duration: Timer::from_seconds(1.5, TimerMode::Once),
        })
        .id()
}
