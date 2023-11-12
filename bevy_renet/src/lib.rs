pub use renet;

use bevy::prelude::*;

use renet::{RenetClient, RenetServer, ServerEvent};

#[cfg(feature = "transport")]
pub mod transport;

// #[cfg(feature = "steam")]
// pub mod steam;

/// This system set is where all transports receive messages
///
/// If you want to ensure data has arrived in the [`RenetClient`] or [`RenetServer`], then schedule your
/// system after this set.
///
/// This system set runs in PreUpdate.
#[derive(Debug, SystemSet, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenetReceive;

/// This system set is where all transports send messages
///
/// If you want to ensure your packets have been registered by the [`RenetClient`] or [`RenetServer`], then
/// schedule your system before this set.
///
/// This system set runs in PostUpdate.
#[derive(Debug, SystemSet, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenetSend;

pub struct RenetServerPlugin;

pub struct RenetClientPlugin;

impl Plugin for RenetServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Events<ServerEvent>>();
        app.add_systems(PreUpdate, Self::update_system.run_if(resource_exists::<RenetServer>()));
    }
}

impl RenetServerPlugin {
    pub fn update_system(mut server: ResMut<RenetServer>, time: Res<Time>, mut server_events: EventWriter<ServerEvent>) {
        server.update(time.delta());

        while let Some(event) = server.get_event() {
            server_events.send(event);
        }
    }
}

impl Plugin for RenetClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, Self::update_system.run_if(resource_exists::<RenetClient>()));
    }
}

impl RenetClientPlugin {
    pub fn update_system(mut client: ResMut<RenetClient>, time: Res<Time>) {
        client.update(time.delta());
    }
}

pub fn client_connected() -> impl FnMut(Option<Res<RenetClient>>) -> bool {
    |client| match client {
        Some(client) => client.is_connected(),
        None => false,
    }
}

pub fn client_disconnected() -> impl FnMut(Option<Res<RenetClient>>) -> bool {
    |client| match client {
        Some(client) => client.is_disconnected(),
        None => true,
    }
}

pub fn client_connecting() -> impl FnMut(Option<Res<RenetClient>>) -> bool {
    |client| match client {
        Some(client) => client.is_connecting(),
        None => false,
    }
}

pub fn client_just_connected() -> impl FnMut(Local<bool>, Option<Res<RenetClient>>) -> bool {
    |mut last_connected: Local<bool>, client| {
        let connected = client.map(|client| client.is_connected()).unwrap_or(false);

        let just_connected = !*last_connected && connected;
        *last_connected = connected;
        just_connected
    }
}

pub fn client_just_disconnected() -> impl FnMut(Local<bool>, Option<Res<RenetClient>>) -> bool {
    |mut last_connected: Local<bool>, client| {
        let disconnected = client.map(|client| client.is_disconnected()).unwrap_or(true);

        let just_disconnected = *last_connected && disconnected;
        *last_connected = !disconnected;
        just_disconnected
    }
}
