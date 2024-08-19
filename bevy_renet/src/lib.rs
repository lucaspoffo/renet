pub use renet;

use bevy::{ecs::schedule::ScheduleLabel, prelude::*, utils::intern::Interned};

use renet::{RenetClient, RenetServer, ServerEvent};

#[cfg(feature = "transport")]
pub mod transport;

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

#[derive(Debug, SystemSet, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreSet {
    Pre,
    Post,
}
#[derive(Clone, Copy)]
pub struct NetSchedules {
    pub pre: Interned<dyn ScheduleLabel>,
    pub post: Interned<dyn ScheduleLabel>,
}
impl Default for NetSchedules {
    fn default() -> Self {
        Self {
            pre: PreUpdate.intern(),
            post: PostUpdate.intern(),
        }
    }
}
#[derive(Default)]
pub struct RenetServerPlugin {
    pub schedules: NetSchedules,
}
#[derive(Default)]
pub struct RenetClientPlugin {
    pub schedules: NetSchedules,
}

impl Plugin for RenetServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Events<ServerEvent>>();
        app.add_systems(
            self.schedules.pre,
            Self::update_system.in_set(CoreSet::Pre).run_if(resource_exists::<RenetServer>),
        );
        app.add_systems(
            self.schedules.pre,
            Self::emit_server_events_system
            .in_set(RenetReceive)
            .run_if(resource_exists::<RenetServer>)
            .in_set(CoreSet::Pre).run_if(resource_exists::<RenetServer>())
                .after(Self::update_system),
        );
    }
}

impl RenetServerPlugin {
    pub fn update_system(mut server: ResMut<RenetServer>, time: Res<Time>) {
        server.update(time.delta());
    }

    pub fn emit_server_events_system(mut server: ResMut<RenetServer>, mut server_events: EventWriter<ServerEvent>) {
        while let Some(event) = server.get_event() {
            server_events.send(event);
        }
    }
}

impl Plugin for RenetClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(self.schedules.pre, Self::update_system.in_set(CoreSet::Pre).run_if(resource_exists::<RenetClient>()));
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
