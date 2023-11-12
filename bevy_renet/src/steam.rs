use bevy::{app::AppExit, prelude::*};
use renet::{RenetClient, RenetServer};
use renet_steam::steamworks::SteamError;

use crate::{RenetClientPlugin, RenetReceive, RenetSend, RenetServerPlugin};

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
                .in_set(RenetReceive)
                .run_if(resource_exists::<RenetServer>())
                .after(RenetServerPlugin::update_system),
        );

        app.add_systems(
            PostUpdate,
            (Self::send_packets.in_set(RenetSend), Self::disconnect_on_exit).run_if(resource_exists::<RenetServer>()),
        );
    }
}

impl SteamServerPlugin {
    fn update_system(mut transport: Option<NonSendMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
        if let Some(transport) = transport.as_mut() {
            transport.update(&mut server);
        }
    }

    fn send_packets(mut transport: Option<NonSendMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
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
                .in_set(RenetReceive)
                .run_if(resource_exists::<SteamClientTransport>())
                .run_if(resource_exists::<RenetClient>())
                .after(RenetClientPlugin::update_system),
        );
        app.add_systems(
            PostUpdate,
            (Self::send_packets.in_set(RenetSend), Self::disconnect_on_exit)
                .run_if(resource_exists::<SteamClientTransport>())
                .run_if(resource_exists::<RenetClient>()),
        );
    }
}

impl SteamClientPlugin {
    fn update_system(mut transport: ResMut<SteamClientTransport>, mut client: ResMut<RenetClient>) {
        transport.update(&mut client);
    }

    fn send_packets(
        mut transport: ResMut<SteamClientTransport>,
        mut client: ResMut<RenetClient>,
        mut transport_errors: EventWriter<SteamTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.send(SteamTransportError(e));
        }
    }

    fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<SteamClientTransport>) {
        if !exit.is_empty() {
            transport.disconnect();
        }
    }
}
