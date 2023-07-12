use renet::{
    transport::{NetcodeClientTransport, NetcodeServerTransport, NetcodeTransportError},
    RenetClient, RenetServer,
};

use bevy::{app::AppExit, prelude::*};

use crate::RenetSet;

/// Set for networking systems.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum TransportSet {
    Client,
    Server,
}

pub struct NetcodeServerPlugin;

pub struct NetcodeClientPlugin;

impl Plugin for NetcodeServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<NetcodeTransportError>();
        app.configure_set(
            Update,
            TransportSet::Server
                .run_if(resource_exists::<NetcodeServerTransport>().and_then(resource_exists::<RenetServer>()))
                .after(RenetSet::Server),
        );

        app.add_systems(PreUpdate, Self::update_system.in_set(TransportSet::Server));
        app.add_systems(
            PostUpdate,
            (Self::send_packets, Self::disconnect_on_exit).in_set(TransportSet::Server),
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
            transport_errors.send(e);
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

        app.configure_set(
            Update,
            TransportSet::Client
                .run_if(resource_exists::<NetcodeClientTransport>().and_then(resource_exists::<RenetClient>()))
                .after(RenetSet::Client),
        );

        app.add_systems(PreUpdate, Self::update_system.in_set(TransportSet::Client));
        app.add_systems(PostUpdate, Self::send_packets.in_set(TransportSet::Client));
        app.add_systems(PostUpdate, Self::disconnect_on_exit.in_set(TransportSet::Client));
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
            transport_errors.send(e);
        }
    }

    pub fn send_packets(
        mut transport: ResMut<NetcodeClientTransport>,
        mut client: ResMut<RenetClient>,
        mut transport_errors: EventWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.send(e);
        }
    }

    fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<NetcodeClientTransport>) {
        if !exit.is_empty() && !transport.is_disconnected() {
            transport.disconnect();
        }
    }
}

pub fn client_connected(transport: Option<Res<NetcodeClientTransport>>) -> bool {
    match transport {
        Some(transport) => transport.is_connected(),
        None => false,
    }
}

pub fn client_diconnected(transport: Option<Res<NetcodeClientTransport>>) -> bool {
    match transport {
        Some(transport) => transport.is_disconnected(),
        None => true,
    }
}

pub fn client_connecting(transport: Option<Res<NetcodeClientTransport>>) -> bool {
    match transport {
        Some(transport) => transport.is_connecting(),
        None => false,
    }
}
