pub use renet;

use bevy::prelude::*;

use renet::{RenetClient, RenetServer, ServerEvent};

#[cfg(feature = "transport")]
pub mod transport;
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
    pub fn update_system(
        mut disconnected: Local<bool>,
        mut client: ResMut<RenetClient>,
        time: Res<Time>,
        mut connect_events: EventWriter<ClientConnected>,
        mut disconnect_events: EventWriter<ClientDisconnected>,
    ) {
        client.update(time.delta());

        if client.is_disconnected() && !*disconnected {
            *disconnected = true;
            disconnect_events.send_default();
        } else if !client.is_disconnected() && *disconnected {
            *disconnected = false;
            connect_events.send_default();
        }
    }
}

#[derive(Default, Event)]
pub struct ClientConnected;

#[derive(Default, Event)]
pub struct ClientDisconnected;
