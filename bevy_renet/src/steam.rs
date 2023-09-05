use bevy::{app::AppExit, prelude::*};
use renet::{RenetClient, RenetServer};
use renet_steam::steamworks::SteamError;

use crate::{RenetClientPlugin, RenetServerPlugin};

pub use renet_steam::{AccessPermission, SteamClientTransport, SteamServerConfig, SteamServerTransport};

pub struct SteamServerPlugin;

pub struct SteamClientPlugin;

#[derive(Debug, Event)]
pub struct SteamTransportError(pub SteamError);

impl std::fmt::Display for SteamTransportError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

impl Plugin for SteamServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<RenetServer>())
                .after(RenetServerPlugin::update_system),
        );

        app.add_systems(
            PostUpdate,
            (Self::send_packets, Self::disconnect_on_exit).run_if(resource_exists::<RenetServer>()),
        );
    }
}

impl SteamServerPlugin {
    pub fn update_system(mut transport: Option<NonSendMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
        if let Some(transport) = transport.as_mut() {
            transport.update(&mut server);
        }
    }

    pub fn send_packets(mut transport: Option<NonSendMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
        if let Some(transport) = transport.as_mut() {
            transport.send_packets(&mut server);
        }
    }

    pub fn disconnect_on_exit(
        exit: EventReader<AppExit>,
        mut transport: Option<NonSendMut<SteamServerTransport>>,
        mut server: ResMut<RenetServer>,
    ) {
        if let Some(transport) = transport.as_mut() {
            if !exit.is_empty() {
                transport.disconnect_all(&mut server, false);
            }
        }
    }
}

impl Plugin for SteamClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SteamTransportError>();

        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<SteamClientTransport>())
                .run_if(resource_exists::<RenetClient>())
                .after(RenetClientPlugin::update_system),
        );
        app.add_systems(
            PostUpdate,
            (Self::send_packets, Self::disconnect_on_exit)
                .run_if(resource_exists::<SteamClientTransport>())
                .run_if(resource_exists::<RenetClient>()),
        );
    }
}

impl SteamClientPlugin {
    pub fn update_system(mut transport: ResMut<SteamClientTransport>, mut client: ResMut<RenetClient>) {
        transport.update(&mut client);
    }

    pub fn send_packets(
        mut transport: ResMut<SteamClientTransport>,
        mut client: ResMut<RenetClient>,
        mut transport_errors: EventWriter<SteamTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.send(SteamTransportError(e));
        }
    }

    fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<SteamClientTransport>) {
        if !exit.is_empty() && !transport.is_disconnected() {
            transport.disconnect();
        }
    }
}

pub fn client_connected() -> impl FnMut(Option<Res<SteamClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => transport.is_connected(),
        None => false,
    }
}

pub fn client_diconnected() -> impl FnMut(Option<Res<SteamClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => transport.is_disconnected(),
        None => true,
    }
}

pub fn client_connecting() -> impl FnMut(Option<Res<SteamClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => transport.is_connecting(),
        None => false,
    }
}

pub fn client_just_connected() -> impl FnMut(Local<bool>, Option<Res<SteamClientTransport>>) -> bool {
    |mut last_connected: Local<bool>, transport| {
        let connected = transport.map(|transport| transport.is_connected()).unwrap_or(false);
        let just_connected = !*last_connected && connected;
        *last_connected = connected;
        just_connected
    }
}

pub fn client_just_diconnected() -> impl FnMut(Local<bool>, Option<Res<SteamClientTransport>>) -> bool {
    |mut last_connected: Local<bool>, transport| {
        let disconnected = transport.map(|transport| transport.is_disconnected()).unwrap_or(true);

        let just_disconnected = *last_connected && disconnected;
        *last_connected = !disconnected;
        just_disconnected
    }
}
