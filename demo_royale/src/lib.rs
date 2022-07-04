use std::time::Duration;

use bevy::prelude::*;
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
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Debug, Serialize, Deserialize, Component)]
pub enum PlayerCommand {
    Input(PlayerInput),
    BasicAttack { cast_at: Vec3 },
}

pub enum Channel {
    Reliable,
    ReliableCritical,
    Unreliable,
}

#[derive(Debug, Serialize, Deserialize, Component)]
pub enum ServerMessages {
    PlayerCreate { entity: Entity, id: u64, translation: [f32; 3] },
    PlayerRemove { id: u64 },
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkedPlayers {
    pub entities: Vec<Entity>,
    pub translations: Vec<[f32; 3]>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkFrame {
    pub players: NetworkedPlayers,
}

impl Channel {
    pub fn id(&self) -> u8 {
        match self {
            Channel::Reliable => 0,
            Channel::ReliableCritical => 1,
            Channel::Unreliable => 2,
        }
    }

    pub fn channels_config() -> Vec<ChannelConfig> {
        vec![
            ReliableChannelConfig {
                channel_id: Channel::Reliable.id(),
                message_resend_time: Duration::from_millis(200),
                ..Default::default()
            }
            .into(),
            ReliableChannelConfig {
                channel_id: Channel::ReliableCritical.id(),
                message_resend_time: Duration::ZERO,
                ..Default::default()
            }
            .into(),
            UnreliableChannelConfig {
                channel_id: Channel::Unreliable.id(),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn connection_config() -> RenetConnectionConfig {
    RenetConnectionConfig {
        channels_config: Channel::channels_config(),
        ..Default::default()
    }
}

/// set up a simple 3D scene
pub fn setup_level(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // plane
    commands.spawn_bundle(PbrBundle {
        mesh: meshes.add(Mesh::from(shape::Box::new(10., 1., 10.))),
        material: materials.add(Color::rgb(0.3, 0.5, 0.3).into()),
        ..Default::default()
    });
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
