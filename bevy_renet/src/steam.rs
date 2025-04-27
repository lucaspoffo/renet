use bevy_app::{prelude::*, AppExit};
use bevy_ecs::prelude::*;
use renet::{RenetClient, RenetServer};
use steamworks::SteamError;

use crate::{RenetClientPlugin, RenetReceive, RenetSend, RenetServerPlugin};

pub use renet_steam::*;

pub struct SteamServerPlugin {
    pub schedules: NetSchedules,
}
pub struct SteamClientPlugin {
    pub schedules: NetSchedules,
}
impl Default for SteamServerPlugin {
    fn default() -> Self {
        Self {
            pre_schedule: PreUpdate.intern(),
            post_schedule: PostUpdate.intern(),
        }
    }
}
impl Default for SteamClientPlugin {
    fn default() -> Self {
        Self {
            pre_schedule: PreUpdate.intern(),
            post_schedule: PostUpdate.intern(),
        }
    }
}
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
            self.schedules.pre,
            Self::update_system
                .in_set(RenetReceive)
                .in_set(CoreSet::Pre)
                .run_if(resource_exists::<RenetServer>)
                .after(RenetServerPlugin::update_system)
                .before(RenetServerPlugin::emit_server_events_system),
        );

        app.add_systems(
            self.schedules.post,
            (Self::send_packets.in_set(RenetSend), Self::disconnect_on_exit)
                .in_set(CoreSet::Post)
                .run_if(resource_exists::<RenetServer>),
        );

        app.add_systems(Last, Self::disconnect_on_exit.run_if(resource_exists::<RenetServer>));
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
            self.schedules.pre,
            Self::update_system
                .in_set(RenetReceive)
                .run_if(resource_exists::<SteamClientTransport>)
                .run_if(resource_exists::<RenetClient>)
                .after(RenetClientPlugin::update_system)
                .in_set(CoreSet::Pre),
        );
        app.add_systems(
            self.schedules.post,
            Self::send_packets
                .in_set(RenetSend)
                .run_if(resource_exists::<SteamClientTransport>)
                .run_if(resource_exists::<RenetClient>)
                .in_set(CoreSet::Post),
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
        mut transport_errors: EventWriter<SteamTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.write(SteamTransportError(e));
        }
    }

    pub fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<SteamClientTransport>) {
        if !exit.is_empty() {
            transport.disconnect();
        }
    }
}
