use std::collections::HashMap;

use bevy::{
    prelude::{shape::Icosphere, *},
    window::PrimaryWindow,
};
use bevy_egui::EguiContexts;
use bevy_renet::renet::RenetClient;
use renet_visualizer::RenetClientVisualizer;
use smooth_bevy_cameras::{LookTransform, LookTransformBundle, Smoother};

use crate::{ClientChannel, PlayerCommand, PlayerInput};

#[derive(Component)]
pub struct Target;

#[derive(Component)]
pub struct ControlledPlayer;

#[derive(Default, Resource)]
pub struct NetworkMapping(pub HashMap<Entity, Entity>);

#[derive(Debug)]
pub struct PlayerInfo {
    pub client_entity: Entity,
    pub server_entity: Entity,
}

#[derive(Debug, Default, Resource)]
pub struct ClientLobby {
    pub players: HashMap<u64, PlayerInfo>,
}

pub fn player_input(
    keyboard_input: Res<Input<KeyCode>>,
    mut player_input: ResMut<PlayerInput>,
    mouse_button_input: Res<Input<MouseButton>>,
    target_query: Query<&Transform, With<Target>>,
    mut player_commands: EventWriter<PlayerCommand>,
) {
    player_input.left = keyboard_input.pressed(KeyCode::A) || keyboard_input.pressed(KeyCode::Left);
    player_input.right = keyboard_input.pressed(KeyCode::D) || keyboard_input.pressed(KeyCode::Right);
    player_input.up = keyboard_input.pressed(KeyCode::W) || keyboard_input.pressed(KeyCode::Up);
    player_input.down = keyboard_input.pressed(KeyCode::S) || keyboard_input.pressed(KeyCode::Down);

    if mouse_button_input.just_pressed(MouseButton::Left) {
        let target_transform = target_query.single();
        player_commands.send(PlayerCommand::BasicAttack {
            cast_at: target_transform.translation,
        });
    }
}

pub fn client_send_input(player_input: Res<PlayerInput>, mut client: ResMut<RenetClient>) {
    let input_message = bincode::serialize(&*player_input).unwrap();
    client.send_message(ClientChannel::Input, input_message);
}

pub fn client_send_player_commands(mut player_commands: EventReader<PlayerCommand>, mut client: ResMut<RenetClient>) {
    for command in player_commands.iter() {
        let command_message = bincode::serialize(command).unwrap();
        client.send_message(ClientChannel::Command, command_message);
    }
}

pub fn update_visulizer_system(
    mut egui_contexts: EguiContexts,
    mut visualizer: ResMut<RenetClientVisualizer<200>>,
    client: Res<RenetClient>,
    mut show_visualizer: Local<bool>,
    keyboard_input: Res<Input<KeyCode>>,
) {
    visualizer.add_network_info(client.network_info());
    if keyboard_input.just_pressed(KeyCode::F1) {
        *show_visualizer = !*show_visualizer;
    }
    if *show_visualizer {
        visualizer.show_window(egui_contexts.ctx_mut());
    }
}

pub fn update_target_system(
    primary_window: Query<&Window, With<PrimaryWindow>>,
    mut target_query: Query<&mut Transform, With<Target>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let (camera, camera_transform) = camera_query.single();
    let mut target_transform = target_query.single_mut();
    if let Some(cursor_pos) = primary_window.single().cursor_position() {
        if let Some(ray) = camera.viewport_to_world(camera_transform, cursor_pos) {
            if let Some(distance) = ray.intersect_plane(Vec3::Y, Vec3::Y) {
                target_transform.translation = ray.direction * distance + ray.origin;
            }
        }
    }
}

pub fn setup_camera(mut commands: Commands) {
    commands
        .spawn(LookTransformBundle {
            transform: LookTransform {
                eye: Vec3::new(0.0, 8., 2.5),
                target: Vec3::new(0.0, 0.5, 0.0),
                up: Vec3::Y,
            },
            smoother: Smoother::new(0.9),
        })
        .insert(Camera3dBundle {
            transform: Transform::from_xyz(0., 8.0, 2.5).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
            ..default()
        });
}

pub fn setup_target(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands
        .spawn(PbrBundle {
            mesh: meshes.add(
                Mesh::try_from(Icosphere {
                    radius: 0.1,
                    subdivisions: 5,
                })
                .unwrap(),
            ),
            material: materials.add(Color::rgb(1.0, 0.0, 0.0).into()),
            transform: Transform::from_xyz(0.0, 0., 0.0),
            ..Default::default()
        })
        .insert(Target);
}

pub fn camera_follow(
    mut camera_query: Query<&mut LookTransform, (With<Camera>, Without<ControlledPlayer>)>,
    player_query: Query<&Transform, With<ControlledPlayer>>,
) {
    let mut cam_transform = camera_query.single_mut();
    if let Ok(player_transform) = player_query.get_single() {
        cam_transform.eye.x = player_transform.translation.x;
        cam_transform.eye.z = player_transform.translation.z + 2.5;
        cam_transform.target = player_transform.translation;
    }
}
