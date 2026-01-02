pub use renet;

use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_time::prelude::*;
use renet::ConnectionConfig;

#[cfg(feature = "netcode")]
pub mod netcode;

#[cfg(feature = "steam")]
pub mod steam;

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
        app.init_resource::<Messages<ServerEvent>>();
        app.add_systems(PreUpdate, Self::update_system.run_if(resource_exists::<RenetServer>));
        app.add_systems(
            PreUpdate,
            Self::emit_server_events_system
                .in_set(RenetReceive)
                .run_if(resource_exists::<RenetServer>)
                .after(Self::update_system),
        );
    }
}

impl RenetServerPlugin {
    pub fn update_system(mut server: ResMut<RenetServer>, time: Res<Time>) {
        server.update(time.delta());
    }

    pub fn emit_server_events_system(mut server: ResMut<RenetServer>, mut server_messages: MessageWriter<ServerEvent>) {
        while let Some(event) = server.get_event() {
            server_messages.write(event.into());
        }
    }
}

impl Plugin for RenetClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, Self::update_system.run_if(resource_exists::<RenetClient>));
    }
}

impl RenetClientPlugin {
    pub fn update_system(mut client: ResMut<RenetClient>, time: Res<Time>) {
        client.update(time.delta());
    }
}

pub fn client_connected(client: Option<Res<RenetClient>>) -> bool {
    match client {
        Some(client) => client.is_connected(),
        None => false,
    }
}

pub fn client_disconnected(client: Option<Res<RenetClient>>) -> bool {
    match client {
        Some(client) => client.is_disconnected(),
        None => true,
    }
}

pub fn client_connecting(client: Option<Res<RenetClient>>) -> bool {
    match client {
        Some(client) => client.is_connecting(),
        None => false,
    }
}

pub fn client_just_connected(mut last_connected: Local<bool>, client: Option<Res<RenetClient>>) -> bool {
    let connected = client.map(|client| client.is_connected()).unwrap_or(false);

    let just_connected = !*last_connected && connected;
    *last_connected = connected;
    just_connected
}

pub fn client_just_disconnected(mut last_connected: Local<bool>, client: Option<Res<RenetClient>>) -> bool {
    let disconnected = client.map(|client| client.is_disconnected()).unwrap_or(true);

    let just_disconnected = *last_connected && disconnected;
    *last_connected = !disconnected;
    just_disconnected
}

#[derive(Resource, Deref, DerefMut, Debug)]
pub struct RenetClient(pub renet::RenetClient);

impl RenetClient {
    pub fn new(connection_config: ConnectionConfig) -> Self {
        Self(renet::RenetClient::new(connection_config))
    }
}

#[derive(Resource, Deref, DerefMut, Debug)]
pub struct RenetServer(pub renet::RenetServer);

impl RenetServer {
    pub fn new(connection_config: ConnectionConfig) -> Self {
        Self(renet::RenetServer::new(connection_config))
    }
}

#[derive(Message, Deref, DerefMut, Debug, PartialEq, Eq)]
pub struct ServerEvent(pub renet::ServerEvent);

impl From<renet::ServerEvent> for ServerEvent {
    fn from(value: renet::ServerEvent) -> Self {
        Self(value)
    }
}
