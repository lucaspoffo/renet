use renet::{RenetClient, RenetServer};
pub use renet_netcode::*;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::prelude::*;

use crate::{RenetClientPlugin, RenetReceive, RenetSend, RenetServerPlugin};

pub struct NetcodeServerPlugin;

pub struct NetcodeClientPlugin;

impl Plugin for NetcodeServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<NetcodeTransportError>();

        app.add_systems(
            PreUpdate,
            Self::update_system
                .in_set(RenetReceive)
                .run_if(resource_exists::<NetcodeServerTransport>)
                .run_if(resource_exists::<RenetServer>)
                .after(RenetServerPlugin::update_system)
                .before(RenetServerPlugin::emit_server_events_system),
        );

        app.add_systems(
            PostUpdate,
            Self::send_packets
                .in_set(RenetSend)
                .run_if(resource_exists::<NetcodeServerTransport>)
                .run_if(resource_exists::<RenetServer>),
        );

        app.add_systems(
            Last,
            Self::disconnect_on_exit
                .run_if(resource_exists::<NetcodeServerTransport>)
                .run_if(resource_exists::<RenetServer>),
        );
    }
}

impl NetcodeServerPlugin {
    pub fn update_system(
        mut transport: ResMut<NetcodeServerTransport>,
        mut server: ResMut<RenetServer>,
        time: Res<Time>,
        mut transport_errors: EventWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.update(time.delta(), &mut server) {
            transport_errors.write(e);
        }
    }

    pub fn send_packets(mut transport: ResMut<NetcodeServerTransport>, mut server: ResMut<RenetServer>) {
        transport.send_packets(&mut server);
    }

    pub fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<NetcodeServerTransport>, mut server: ResMut<RenetServer>) {
        if !exit.is_empty() {
            transport.disconnect_all(&mut server);
        }
    }
}

impl Plugin for NetcodeClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<NetcodeTransportError>();

        app.add_systems(
            PreUpdate,
            Self::update_system
                .in_set(RenetReceive)
                .run_if(resource_exists::<NetcodeClientTransport>)
                .run_if(resource_exists::<RenetClient>)
                .after(RenetClientPlugin::update_system),
        );
        app.add_systems(
            PostUpdate,
            Self::send_packets
                .in_set(RenetSend)
                .run_if(resource_exists::<NetcodeClientTransport>)
                .run_if(resource_exists::<RenetClient>),
        );

        app.add_systems(
            Last,
            Self::disconnect_on_exit
                .run_if(resource_exists::<NetcodeClientTransport>)
                .run_if(resource_exists::<RenetClient>),
        );
    }
}

impl NetcodeClientPlugin {
    pub fn update_system(
        mut transport: ResMut<NetcodeClientTransport>,
        mut client: ResMut<RenetClient>,
        time: Res<Time>,
        mut transport_errors: EventWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.update(time.delta(), &mut client) {
            transport_errors.write(e);
        }
    }

    pub fn send_packets(
        mut transport: ResMut<NetcodeClientTransport>,
        mut client: ResMut<RenetClient>,
        mut transport_errors: EventWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.write(e);
        }
    }

    pub fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<NetcodeClientTransport>) {
        if !exit.is_empty() {
            transport.disconnect();
        }
    }
}
