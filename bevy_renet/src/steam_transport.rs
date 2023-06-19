use bevy::{
    prelude::{resource_exists, App, Condition, CoreSet, IntoSystemConfig, IntoSystemSetConfig, Plugin, Res, ResMut, SystemSet},
    time::Time,
};
use renet::{RenetClient, RenetServer};
use renet_steam_transport::transport::{client::SteamClientTransport, server::SteamServerTransport, Transport};
use steamworks::ClientManager;

use crate::RenetSet;

/// Set for networking systems.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum TransportSet {
    Client,
    Server,
}
pub struct SteamServerPlugin;

pub struct SteamClientPlugin;

impl Plugin for SteamServerPlugin {
    fn build(&self, app: &mut App) {
        app.configure_set(
            TransportSet::Server
                .run_if(resource_exists::<SteamServerTransport<ClientManager>>().and_then(resource_exists::<RenetServer>()))
                .after(RenetSet::Server),
        );
        app.add_system(Self::update_system.in_base_set(CoreSet::PreUpdate).in_set(TransportSet::Server));
        app.add_system(Self::send_packets.in_base_set(CoreSet::PostUpdate).in_set(TransportSet::Server));
    }
}

impl SteamServerPlugin {
    pub fn update_system(mut transport: ResMut<SteamServerTransport<ClientManager>>, mut server: ResMut<RenetServer>, time: Res<Time>) {
        transport.update(time.delta(), &mut server)
    }

    pub fn send_packets(mut transport: ResMut<SteamServerTransport<ClientManager>>, mut server: ResMut<RenetServer>) {
        transport.send_packets(&mut server);
    }
}

impl Plugin for SteamClientPlugin {
    fn build(&self, app: &mut App) {
        app.configure_set(
            TransportSet::Client
                .run_if(resource_exists::<SteamClientTransport>().and_then(resource_exists::<RenetClient>()))
                .after(RenetSet::Client),
        );
        app.add_system(Self::update_system.in_base_set(CoreSet::PreUpdate).in_set(TransportSet::Client));
        app.add_system(Self::send_packets.in_base_set(CoreSet::PostUpdate).in_set(TransportSet::Client));
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
