use std::fmt::{self, Display, Formatter};

pub use renet_steam::*;

use bevy_app::{prelude::*, AppExit};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use steamworks::{networking_sockets::InvalidHandle, Client, SteamError, SteamId};

use crate::{RenetClient, RenetClientPlugin, RenetReceive, RenetSend, RenetServer, RenetServerPlugin};

pub struct SteamServerPlugin;

pub struct SteamClientPlugin;

impl Plugin for SteamServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            Self::update_system
                .in_set(RenetReceive)
                .run_if(resource_exists::<RenetServer>)
                .after(RenetServerPlugin::update_system)
                .before(RenetServerPlugin::emit_server_events_system),
        );

        app.add_systems(
            PostUpdate,
            Self::send_packets.in_set(RenetSend).run_if(resource_exists::<RenetServer>),
        );

        app.add_systems(Last, Self::disconnect_on_exit.run_if(resource_exists::<RenetServer>));
    }
}

impl SteamServerPlugin {
    pub fn update_system(mut transport: Option<NonSendMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
        if let Some(transport) = transport.as_mut() {
            transport.update(&mut **server);
        }
    }

    pub fn send_packets(mut transport: Option<NonSendMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
        if let Some(transport) = transport.as_mut() {
            transport.send_packets(&mut *server);
        }
    }

    pub fn disconnect_on_exit(
        exit: MessageReader<AppExit>,
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
        app.add_message::<SteamTransportError>();

        app.add_systems(
            PreUpdate,
            Self::update_system
                .in_set(RenetReceive)
                .run_if(resource_exists::<SteamClientTransport>)
                .run_if(resource_exists::<RenetClient>)
                .after(RenetClientPlugin::update_system),
        );
        app.add_systems(
            PostUpdate,
            Self::send_packets
                .in_set(RenetSend)
                .run_if(resource_exists::<SteamClientTransport>)
                .run_if(resource_exists::<RenetClient>),
        );

        app.add_systems(
            Last,
            Self::disconnect_on_exit
                .run_if(resource_exists::<SteamClientTransport>)
                .run_if(resource_exists::<RenetClient>),
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
        mut transport_errors: MessageWriter<SteamTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.write(SteamTransportError(e));
        }
    }

    pub fn disconnect_on_exit(exit: MessageReader<AppExit>, mut transport: ResMut<SteamClientTransport>) {
        if !exit.is_empty() {
            transport.disconnect();
        }
    }
}

#[derive(Resource, Deref, DerefMut)]
pub struct SteamClientTransport(pub renet_steam::SteamClientTransport);

impl SteamClientTransport {
    pub fn new(client: Client, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        renet_steam::SteamClientTransport::new(client, steam_id).map(Self)
    }
}

#[derive(Resource, Deref, DerefMut)]
pub struct SteamServerTransport(pub renet_steam::SteamServerTransport);

impl SteamServerTransport {
    pub fn new(client: Client, config: SteamServerConfig) -> Result<Self, InvalidHandle> {
        renet_steam::SteamServerTransport::new(client, config).map(Self)
    }
}

#[derive(Debug, Message, Deref)]
pub struct SteamTransportError(pub SteamError);

impl Display for SteamTransportError {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0)
    }
}
