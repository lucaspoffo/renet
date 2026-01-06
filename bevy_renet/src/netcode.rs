use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io,
    net::UdpSocket,
    time::Duration,
};

pub use renet_netcode::*;

use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_time::prelude::*;

use crate::{RenetClient, RenetClientPlugin, RenetReceive, RenetSend, RenetServer, RenetServerPlugin};

pub struct NetcodeServerPlugin;

pub struct NetcodeClientPlugin;

impl Plugin for NetcodeServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<NetcodeTransportError>();

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
        mut transport_errors: MessageWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.update(time.delta(), &mut server) {
            transport_errors.write(e.into());
        }
    }

    pub fn send_packets(mut transport: ResMut<NetcodeServerTransport>, mut server: ResMut<RenetServer>) {
        transport.send_packets(&mut server);
    }

    pub fn disconnect_on_exit(
        exit: MessageReader<AppExit>,
        mut transport: ResMut<NetcodeServerTransport>,
        mut server: ResMut<RenetServer>,
    ) {
        if !exit.is_empty() {
            transport.disconnect_all(&mut server);
        }
    }
}

impl Plugin for NetcodeClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<NetcodeTransportError>();

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
        mut transport_errors: MessageWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.update(time.delta(), &mut client) {
            transport_errors.write(e.into());
        }
    }

    pub fn send_packets(
        mut transport: ResMut<NetcodeClientTransport>,
        mut client: ResMut<RenetClient>,
        mut transport_errors: MessageWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.write(e.into());
        }
    }

    pub fn disconnect_on_exit(exit: MessageReader<AppExit>, mut transport: ResMut<NetcodeClientTransport>) {
        if !exit.is_empty() {
            transport.disconnect();
        }
    }
}

#[derive(Resource, Deref, DerefMut, Debug)]
pub struct NetcodeClientTransport(pub renet_netcode::NetcodeClientTransport);

impl NetcodeClientTransport {
    pub fn new(current_time: Duration, authentication: ClientAuthentication, socket: UdpSocket) -> Result<Self, NetcodeError> {
        renet_netcode::NetcodeClientTransport::new(current_time, authentication, socket).map(Self)
    }
}

#[derive(Resource, Deref, DerefMut)]
pub struct NetcodeServerTransport(pub renet_netcode::NetcodeServerTransport);

impl NetcodeServerTransport {
    pub fn new(server_config: ServerConfig, socket: UdpSocket) -> Result<Self, io::Error> {
        renet_netcode::NetcodeServerTransport::new(server_config, socket).map(Self)
    }
}

#[derive(Message, Debug)]
pub enum NetcodeTransportError {
    Netcode(NetcodeError),
    Renet(renet::DisconnectReason),
    IO(std::io::Error),
}

impl Error for NetcodeTransportError {}

impl Display for NetcodeTransportError {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        match self {
            Self::Netcode(err) => err.fmt(fmt),
            Self::Renet(err) => err.fmt(fmt),
            Self::IO(err) => err.fmt(fmt),
        }
    }
}

impl From<renet_netcode::NetcodeTransportError> for NetcodeTransportError {
    fn from(value: renet_netcode::NetcodeTransportError) -> Self {
        match value {
            renet_netcode::NetcodeTransportError::Netcode(netcode_error) => Self::Netcode(netcode_error),
            renet_netcode::NetcodeTransportError::Renet(disconnect_reason) => Self::Renet(disconnect_reason),
            renet_netcode::NetcodeTransportError::IO(error) => Self::IO(error),
        }
    }
}
