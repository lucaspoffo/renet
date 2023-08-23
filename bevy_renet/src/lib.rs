pub use renet;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::prelude::*;

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
    pub fn update_system(mut client: ResMut<RenetClient>, time: Res<Time>) {
        client.update(time.delta());
    }
}
