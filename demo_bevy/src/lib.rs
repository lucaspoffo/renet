use std::time::Duration;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use bevy_renet::renet::{ChannelConfig, ReliableChannelConfig, RenetConnectionConfig, UnreliableChannelConfig, NETCODE_KEY_BYTES};
use serde::{Deserialize, Serialize};

pub const PRIVATE_KEY: &[u8; NETCODE_KEY_BYTES] = b"an example very very secret key."; // 32-bytes
pub const PROTOCOL_ID: u64 = 7;

#[derive(Debug, Component)]
pub struct Player {
    pub id: u64,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Component)]
pub struct PlayerInput {
    pub most_recent_tick: Option<u32>,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Debug, Serialize, Deserialize, Component)]
pub enum PlayerCommand {
    BasicAttack { cast_at: Vec3 },
}

pub enum ClientChannel {
    Input,
    Command,
}

pub enum ServerChannel {
    ServerMessages,
    NetworkFrame,
}

#[derive(Debug, Serialize, Deserialize, Component)]
pub enum ServerMessages {
    PlayerCreate { entity: Entity, id: u64, translation: [f32; 3] },
    PlayerRemove { id: u64 },
    SpawnProjectile { entity: Entity, translation: [f32; 3] },
    DespawnProjectile { entity: Entity },
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkedEntities {
    pub entities: Vec<Entity>,
    pub translations: Vec<[f32; 3]>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkFrame {
    pub tick: u32,
    pub entities: NetworkedEntities,
}

impl ClientChannel {
    pub fn id(&self) -> u8 {
        match self {
            Self::Input => 0,
            Self::Command => 1,
        }
    }

    pub fn channels_config() -> Vec<ChannelConfig> {
        vec![
            ReliableChannelConfig {
                channel_id: Self::Input.id(),
                message_resend_time: Duration::ZERO,
                ..Default::default()
            }
            .into(),
            ReliableChannelConfig {
                channel_id: Self::Command.id(),
                message_resend_time: Duration::ZERO,
                ..Default::default()
            }
            .into(),
        ]
    }
}

impl ServerChannel {
    pub fn id(&self) -> u8 {
        match self {
            Self::NetworkFrame => 0,
            Self::ServerMessages => 1,
        }
    }

    pub fn channels_config() -> Vec<ChannelConfig> {
        vec![
            UnreliableChannelConfig {
                channel_id: Self::NetworkFrame.id(),
                ..Default::default()
            }
            .into(),
            ReliableChannelConfig {
                channel_id: Self::ServerMessages.id(),
                message_resend_time: Duration::from_millis(200),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn client_connection_config() -> RenetConnectionConfig {
    RenetConnectionConfig {
        send_channels_config: ClientChannel::channels_config(),
        receive_channels_config: ServerChannel::channels_config(),
        ..Default::default()
    }
}

pub fn server_connection_config() -> RenetConnectionConfig {
    RenetConnectionConfig {
        send_channels_config: ServerChannel::channels_config(),
        receive_channels_config: ClientChannel::channels_config(),
        ..Default::default()
    }
}

/// set up a simple 3D scene
pub fn setup_level(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // plane
    commands
        .spawn_bundle(PbrBundle {
            mesh: meshes.add(Mesh::from(shape::Box::new(10., 1., 10.))),
            material: materials.add(Color::rgb(0.3, 0.5, 0.3).into()),
            transform: Transform::from_xyz(0.0, -1.0, 0.0),
            ..Default::default()
        })
        .insert(Collider::cuboid(5., 0.5, 5.));
    // light
    commands.spawn_bundle(PointLightBundle {
        point_light: PointLight {
            intensity: 1500.0,
            shadows_enabled: true,
            ..Default::default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..Default::default()
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
        .spawn_bundle(PbrBundle {
            mesh: meshes.add(Mesh::from(shape::Icosphere {
                radius: 0.1,
                subdivisions: 5,
            })),
            material: materials.add(Color::rgb(1.0, 0.0, 0.0).into()),
            transform: Transform::from_translation(translation),
            ..Default::default()
        })
        .insert(RigidBody::Dynamic)
        .insert(LockedAxes::ROTATION_LOCKED | LockedAxes::TRANSLATION_LOCKED_Y)
        .insert(Collider::ball(0.1))
        .insert(Velocity::linear(direction * 10.))
        .insert(ActiveEvents::COLLISION_EVENTS)
        .insert(Projectile {
            duration: Timer::from_seconds(1.5, false),
        })
        .id()
}

/// A 3D ray, with an origin and direction. The direction is guaranteed to be normalized.
#[derive(Debug, PartialEq, Copy, Clone, Default)]
pub struct Ray3d {
    pub(crate) origin: Vec3,
    pub(crate) direction: Vec3,
}

impl Ray3d {
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Ray3d { origin, direction }
    }

    pub fn from_screenspace(
        windows: &Res<Windows>,
        images: &Res<Assets<Image>>,
        camera: &Camera,
        camera_transform: &GlobalTransform,
    ) -> Option<Self> {
        let view = camera_transform.compute_matrix();
        let screen_size = match camera.target.get_logical_size(windows, images) {
            Some(s) => s,
            None => {
                error!("Unable to get screen size for RenderTarget {:?}", camera.target);
                return None;
            }
        };

        let window = windows.get_primary().unwrap();
        let cursor_position = match window.cursor_position() {
            Some(c) => c,
            None => return None,
        };

        let projection = camera.projection_matrix;

        // 2D Normalized device coordinate cursor position from (-1, -1) to (1, 1)
        let cursor_ndc = (cursor_position / screen_size) * 2.0 - Vec2::from([1.0, 1.0]);
        let ndc_to_world: Mat4 = view * projection.inverse();
        let world_to_ndc = projection * view;
        let is_orthographic = projection.w_axis[3] == 1.0;

        // Compute the cursor position at the near plane. The bevy camera looks at -Z.
        let ndc_near = world_to_ndc.transform_point3(-Vec3::Z * camera.near).z;
        let cursor_pos_near = ndc_to_world.transform_point3(cursor_ndc.extend(ndc_near));

        // Compute the ray's direction depending on the projection used.
        let ray_direction = match is_orthographic {
            true => view.transform_vector3(-Vec3::Z), // All screenspace rays are parallel in ortho
            false => cursor_pos_near - camera_transform.translation, // Direction from camera to cursor
        };

        Some(Ray3d::new(cursor_pos_near, ray_direction))
    }

    pub fn intersect_y_plane(&self, y_offset: f32) -> Option<Vec3> {
        let plane_normal = Vec3::Y;
        let plane_origin = Vec3::new(0.0, y_offset, 0.0);
        let denominator = self.direction.dot(plane_normal);
        if denominator.abs() > f32::EPSILON {
            let point_to_point = plane_origin * y_offset - self.origin;
            let intersect_dist = plane_normal.dot(point_to_point) / denominator;
            let intersect_position = self.direction * intersect_dist + self.origin;
            Some(intersect_position)
        } else {
            None
        }
    }
}
