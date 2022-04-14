pub use renet;

use bevy::{
    ecs::{schedule::ShouldRun, system::Resource},
    prelude::*,
};

use renet::{RenetClient, RenetServer, ServerEvent};

// TODO: error handling
// TODO: user defined update/send rate

pub struct RenetServerPlugin;

pub struct RenetClientPlugin;

impl Plugin for RenetServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ServerEvent>()
            .add_system_to_stage(
                CoreStage::PreUpdate,
                renet_server_update.with_run_criteria(has_resource::<RenetServer>),
            )
            .add_system_to_stage(
                CoreStage::PostUpdate,
                renet_server_send_packets.with_run_criteria(has_resource::<RenetServer>),
            );
    }
}

impl Plugin for RenetClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_system_to_stage(
            CoreStage::PreUpdate,
            renet_client_update.with_run_criteria(has_resource::<RenetClient>),
        )
        .add_system_to_stage(
            CoreStage::PostUpdate,
            renet_client_send_packets.with_run_criteria(has_resource::<RenetClient>),
        );
    }
}

fn has_resource<T: Resource>(resource: Option<Res<T>>) -> ShouldRun {
    match resource.is_some() {
        true => ShouldRun::Yes,
        false => ShouldRun::No,
    }
}

fn renet_server_update(mut server: ResMut<RenetServer>, time: Res<Time>, mut server_events: EventWriter<ServerEvent>) {
    _ = server.update(time.delta());

    while let Some(event) = server.get_event() {
        server_events.send(event);
    }
}

fn renet_server_send_packets(mut server: ResMut<RenetServer>) {
    _ = server.send_packets();
}

fn renet_client_update(mut client: ResMut<RenetClient>, time: Res<Time>) {
    _ = client.update(time.delta());
}

fn renet_client_send_packets(mut client: ResMut<RenetClient>) {
    _ = client.send_packets();
}

pub fn run_if_client_conected(client: Option<Res<RenetClient>>) -> ShouldRun {
    match client {
        Some(client) if client.is_connected() => ShouldRun::Yes,
        _ => ShouldRun::No,
    }
}
