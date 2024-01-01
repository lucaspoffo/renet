use renet::{
    transport::{NetcodeClientTransport, NetcodeServerTransport, NetcodeTransportError},
    RenetClient, RenetServer,
};

use bevy::{app::AppExit, ecs::schedule::ScheduleLabel, prelude::*, utils::intern::Interned};

use crate::{CoreSet, RenetClientPlugin, RenetReceive, RenetSend, RenetServerPlugin};

pub struct NetcodeServerPlugin {
    pub pre_schedule: Interned<dyn ScheduleLabel>,
    pub post_schedule: Interned<dyn ScheduleLabel>,
}

pub struct NetcodeClientPlugin {
    pub pre_schedule: Interned<dyn ScheduleLabel>,
    pub post_schedule: Interned<dyn ScheduleLabel>,
}

impl Default for NetcodeServerPlugin {
    fn default() -> Self {
        Self {
            pre_schedule: PreUpdate.intern(),
            post_schedule: PostUpdate.intern(),
        }
    }
}
impl Default for NetcodeClientPlugin {
    fn default() -> Self {
        Self {
            pre_schedule: PreUpdate.intern(),
            post_schedule: PostUpdate.intern(),
        }
    }
}

impl Plugin for NetcodeServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<NetcodeTransportError>();

        app.add_systems(
            self.pre_schedule,
            Self::update_system
                .in_set(RenetReceive)
                .in_set(CoreSet::Pre)
                .run_if(resource_exists::<NetcodeServerTransport>())
                .run_if(resource_exists::<RenetServer>())
                .after(RenetServerPlugin::update_system)
                .before(RenetServerPlugin::emit_server_events_system),
        );

        app.add_systems(
            self.post_schedule,
            (Self::send_packets.in_set(RenetSend), Self::disconnect_on_exit)
                .in_set(CoreSet::Post)
                .run_if(resource_exists::<NetcodeServerTransport>())
                .run_if(resource_exists::<RenetServer>()),
        );
    }
}

impl NetcodeServerPlugin {
    fn update_system(
        mut transport: ResMut<NetcodeServerTransport>,
        mut server: ResMut<RenetServer>,
        time: Res<Time>,
        mut transport_errors: EventWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.update(time.delta(), &mut server) {
            transport_errors.send(e);
        }
    }

    fn send_packets(mut transport: ResMut<NetcodeServerTransport>, mut server: ResMut<RenetServer>) {
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
            self.pre_schedule,
            Self::update_system
                .in_set(RenetReceive)
                .in_set(CoreSet::Pre)
                .run_if(resource_exists::<NetcodeClientTransport>())
                .run_if(resource_exists::<RenetClient>())
                .after(RenetClientPlugin::update_system),
        );
        app.add_systems(
            self.post_schedule,
            (Self::send_packets.in_set(RenetSend), Self::disconnect_on_exit)
                .in_set(CoreSet::Post)
                .run_if(resource_exists::<NetcodeClientTransport>())
                .run_if(resource_exists::<RenetClient>()),
        );
    }
}

impl NetcodeClientPlugin {
    fn update_system(
        mut transport: ResMut<NetcodeClientTransport>,
        mut client: ResMut<RenetClient>,
        time: Res<Time>,
        mut transport_errors: EventWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.update(time.delta(), &mut client) {
            transport_errors.send(e);
        }
    }

    fn send_packets(
        mut transport: ResMut<NetcodeClientTransport>,
        mut client: ResMut<RenetClient>,
        mut transport_errors: EventWriter<NetcodeTransportError>,
    ) {
        if let Err(e) = transport.send_packets(&mut client) {
            transport_errors.send(e);
        }
    }

    fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<NetcodeClientTransport>) {
        if !exit.is_empty() {
            transport.disconnect();
        }
    }
}
