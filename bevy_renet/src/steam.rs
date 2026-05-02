use std::{
    fmt::{self, Display, Formatter},
    net::SocketAddr,
};

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
    pub fn update_system(mut transport: Option<ResMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
        if let Some(transport) = transport.as_mut() {
            transport.update(&mut server);
        }
    }

    pub fn send_packets(mut transport: Option<ResMut<SteamServerTransport>>, mut server: ResMut<RenetServer>) {
        if let Some(transport) = transport.as_mut() {
            transport.send_packets(&mut server);
        }
    }

    pub fn disconnect_on_exit(
        exit: MessageReader<AppExit>,
        mut transport: Option<ResMut<SteamServerTransport>>,
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

    pub fn send_packets(mut commands: Commands, mut transport: ResMut<SteamClientTransport>, mut client: ResMut<RenetClient>) {
        if let Err(e) = transport.send_packets(&mut client) {
            commands.trigger(SteamErrorEvent(e));
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
    /// Connects to a server using its [`SocketAddr`]
    pub fn new_ip(client: Client, socket_addr: SocketAddr) -> Result<Self, InvalidHandle> {
        renet_steam::SteamClientTransport::new_ip(client, socket_addr).map(Self)
    }

    /// Connects to a server using its steam id
    pub fn new_p2p(client: Client, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        renet_steam::SteamClientTransport::new_p2p(client, steam_id).map(Self)
    }

    /// Connects to a server using its steam id
    ///
    /// Allows for additional connection configuration via the [`SteamClientTransportConfig`]
    pub fn new_p2p_with_config(
        client: steamworks::Client,
        steam_id: &SteamId,
        config: SteamClientTransportConfig,
    ) -> Result<Self, InvalidHandle> {
        renet_steam::SteamClientTransport::new_p2p_with_config(client, steam_id, config).map(Self)
    }

    /// Connects to a server using its [`SocketAddr`]
    ///
    /// Allows for additional connection configuration via the [`SteamClientTransportConfig`]
    pub fn new_ip_with_config(
        client: steamworks::Client,
        socket_addr: SocketAddr,
        config: SteamClientTransportConfig,
    ) -> Result<Self, InvalidHandle> {
        renet_steam::SteamClientTransport::new_ip_with_config(client, socket_addr, config).map(Self)
    }
}

#[derive(Resource, Deref, DerefMut)]
/// A resource wrapper around [`renet_steam::SteamServerTransport`]. Add this resource to the app
/// to enable the `ServerPlugin`'s functionality.
pub struct SteamServerTransport(pub renet_steam::SteamServerTransport);

impl SteamServerTransport {
    /// Creates a new Steam Server that runs off the host's steam id
    ///
    /// * socket_options - Additional configuration to add to your steam server. `Default::default`
    ///   works for most usecases.
    pub fn new(client: Client, config: SteamServerConfig, socket_options: SteamServerSocketOptions) -> Result<Self, InvalidHandle> {
        renet_steam::SteamServerTransport::new(client, config, socket_options).map(Self)
    }

    /// Creates a new Steam Server that does not use a steam id for connections. You need to
    /// connect to this server via its IP address.
    ///
    /// * socket_options - Additional configuration to add to your steam server. `Default::default`
    ///   works for most usecases.
    pub fn new_dedicated_server(
        server: steamworks::Server,
        client: Client,
        config: SteamServerConfig,
        socket_options: SteamServerSocketOptions,
    ) -> Result<Self, InvalidHandle> {
        renet_steam::SteamServerTransport::new_dedicated_server(server, client, config, socket_options).map(Self)
    }
}

#[derive(Event, Debug, Deref)]
pub struct SteamErrorEvent(pub SteamError);

impl Display for SteamErrorEvent {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0)
    }
}
