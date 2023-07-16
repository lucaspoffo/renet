use bevy::prelude::*;
use renet::{RenetClient, RenetServer};
use renet_steam_transport::{client::SteamClientTransport, server::SteamServerTransport};
use std::marker::PhantomData;
use steamworks::Manager;

use crate::{RenetClientPlugin, RenetServerPlugin};

#[cfg(feature = "steam_transport")]
#[derive(Default)]
pub struct SteamServerPlugin<T>(PhantomData<T>);

#[cfg(feature = "steam_transport")]
pub struct SteamClientPlugin;

impl<T: Manager + Sync + Send + 'static> Plugin for SteamServerPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<SteamServerTransport<T>>())
                .run_if(resource_exists::<RenetServer>())
                .after(RenetServerPlugin::update_system),
        );
        app.add_systems(
            PostUpdate,
            Self::send_packets
                .run_if(resource_exists::<SteamServerTransport<T>>())
                .run_if(resource_exists::<RenetServer>()),
        );
    }
}

impl<T: Manager + Sync + Send + 'static> SteamServerPlugin<T> {
    pub fn update_system(mut transport: ResMut<SteamServerTransport<T>>, mut server: ResMut<RenetServer>, time: Res<Time>) {
        transport.update(time.delta(), &mut server)
    }

    pub fn send_packets(mut transport: ResMut<SteamServerTransport<T>>, mut server: ResMut<RenetServer>) {
        transport.send_packets(&mut server);
    }
}

/// Configure the client transport to run only if the client is connected.
impl Plugin for SteamClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<SteamClientTransport>())
                .run_if(resource_exists::<RenetClient>())
                .after(RenetClientPlugin::update_system),
        );
        app.add_systems(
            PostUpdate,
            Self::send_packets
                .run_if(resource_exists::<SteamClientTransport>())
                .run_if(resource_exists::<RenetClient>()),
        );
    }
}

impl SteamClientPlugin {
    pub fn update_system(mut transport: ResMut<SteamClientTransport>, mut client: ResMut<RenetClient>, time: Res<Time>) {
        transport.update(time.delta(), &mut client)
    }

    pub fn send_packets(mut transport: ResMut<SteamClientTransport>, mut client: ResMut<RenetClient>) {
        transport.send_packets(&mut client);
    }
}

pub fn client_connected(transport: Option<Res<SteamClientTransport>>) -> bool {
    match transport {
        Some(transport) => transport.is_connected(),
        None => false,
    }
}

pub fn client_diconnected(transport: Option<Res<SteamClientTransport>>) -> bool {
    match transport {
        Some(transport) => transport.is_disconnected(),
        None => true,
    }
}

pub fn client_connecting(transport: Option<Res<SteamClientTransport>>) -> bool {
    match transport {
        Some(transport) => transport.is_connecting(),
        None => false,
    }
}
