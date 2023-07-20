use renet::{
    transport::{NetcodeClientTransport, NetcodeServerTransport, NetcodeTransportError},
    RenetClient, RenetServer,
};

use bevy::{app::AppExit, prelude::*};

use crate::{RenetClientPlugin, RenetServerPlugin};

pub struct NetcodeServerPlugin;

pub struct NetcodeClientPlugin;

impl Plugin for NetcodeServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<NetcodeTransportError>();

        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<NetcodeServerTransport>())
                .run_if(resource_exists::<RenetServer>())
                .after(RenetServerPlugin::update_system),
        );

        app.add_systems(
            PostUpdate,
            (Self::send_packets, Self::disconnect_on_exit)
                .run_if(resource_exists::<NetcodeServerTransport>())
                .run_if(resource_exists::<RenetServer>()),
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

        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<NetcodeClientTransport>())
                .run_if(resource_exists::<RenetClient>())
                .after(RenetClientPlugin::update_system),
        );
        app.add_systems(
            PostUpdate,
            (Self::send_packets, Self::disconnect_on_exit)
                .run_if(resource_exists::<NetcodeClientTransport>())
                .run_if(resource_exists::<RenetClient>()),
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

pub fn client_connected() -> impl FnMut(Option<Res<NetcodeClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => transport.is_connected(),
        None => false,
    }
}

pub fn client_diconnected() -> impl FnMut(Option<Res<NetcodeClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => transport.is_disconnected(),
        None => true,
    }
}

pub fn client_connecting() -> impl FnMut(Option<Res<NetcodeClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => transport.is_connecting(),
        None => false,
    }
}

pub fn client_just_connected() -> impl FnMut(Local<bool>, Option<Res<NetcodeClientTransport>>) -> bool {
    |mut last_connected: Local<bool>, transport| {
        let Some(transport) = transport else {
           return false;
        };

        let connected = transport.is_connected();
        let just_connected = !*last_connected && connected;
        *last_connected = connected;
        just_connected
    }
}

pub fn client_just_diconnected() -> impl FnMut(Local<bool>, Option<Res<NetcodeClientTransport>>) -> bool {
    |mut last_disconnected: Local<bool>, transport| {
        let Some(transport) = transport else {
           return true;
        };

        let disconnected = transport.is_disconnected();
        let just_disconnected = !*last_disconnected && disconnected;
        *last_disconnected = disconnected;
        just_disconnected
    }
}
