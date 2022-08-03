pub use renet;

use bevy::{
    ecs::{schedule::ShouldRun, system::Resource},
    prelude::*,
};

use renet::{RenetClient, RenetError, RenetServer, ServerEvent};

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemLabel)]
pub enum RenetSystems {
    Update,
    SendPackets,
    ClearEvents,
}

pub struct RenetServerPlugin {
    pub default_system_setup: bool,
}

impl Default for RenetServerPlugin {
    fn default() -> Self {
        Self {
            default_system_setup: true,
        }
    }
}

pub struct RenetClientPlugin {
    pub default_system_setup: bool,
}

impl Default for RenetClientPlugin {
    fn default() -> Self {
        Self {
            default_system_setup: true,
        }
    }
}

impl Plugin for RenetServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Events::<ServerEvent>::default());
        app.insert_resource(Events::<RenetError>::default());

        if self.default_system_setup {
            app.add_system_set_to_stage(CoreStage::PreUpdate, Self::get_systems(RenetSystems::Update))
                .add_system_set_to_stage(
                    CoreStage::PreUpdate,
                    Self::get_systems(RenetSystems::ClearEvents).before(RenetSystems::Update),
                )
                .add_system_set_to_stage(CoreStage::PostUpdate, Self::get_systems(RenetSystems::SendPackets));
        }
    }
}

impl Plugin for RenetClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Events::<RenetError>::default());

        if self.default_system_setup {
            app.add_system_set_to_stage(CoreStage::PreUpdate, Self::get_systems(RenetSystems::Update))
                .add_system_set_to_stage(
                    CoreStage::PreUpdate,
                    Self::get_systems(RenetSystems::ClearEvents).before(RenetSystems::Update),
                )
                .add_system_set_to_stage(CoreStage::PostUpdate, Self::get_systems(RenetSystems::SendPackets));
        }
    }
}

impl RenetServerPlugin {
    pub fn update_system(
        mut server: ResMut<RenetServer>,
        mut renet_error: EventWriter<RenetError>,
        time: Res<Time>,
        mut server_events: EventWriter<ServerEvent>,
    ) {
        if let Err(e) = server.update(time.delta()) {
            renet_error.send(RenetError::IO(e));
        }

        while let Some(event) = server.get_event() {
            server_events.send(event);
        }
    }

    pub fn send_packets(mut server: ResMut<RenetServer>, mut renet_error: EventWriter<RenetError>) {
        if let Err(e) = server.send_packets() {
            renet_error.send(RenetError::IO(e));
        }
    }

    pub fn with_default_system_setup(mut self, default_system_setup: bool) -> Self {
        self.default_system_setup = default_system_setup;
        self
    }

    pub fn get_systems(label: RenetSystems) -> SystemSet {
        match label {
            RenetSystems::ClearEvents => SystemSet::new()
                .with_system(Events::<ServerEvent>::update_system)
                .with_system(Events::<RenetError>::update_system)
                .label(RenetSystems::ClearEvents),
            RenetSystems::Update => SystemSet::new()
                .with_system(Self::update_system.with_run_criteria(has_resource::<RenetServer>))
                .label(RenetSystems::Update),
            RenetSystems::SendPackets => SystemSet::new().with_system(
                Self::send_packets
                    .with_run_criteria(has_resource::<RenetServer>)
                    .label(RenetSystems::SendPackets),
            ),
        }
    }
}

impl RenetClientPlugin {
    pub fn update_system(mut client: ResMut<RenetClient>, mut renet_error: EventWriter<RenetError>, time: Res<Time>) {
        if let Err(e) = client.update(time.delta()) {
            renet_error.send(e);
        }
    }

    pub fn send_packets(mut client: ResMut<RenetClient>, mut renet_error: EventWriter<RenetError>) {
        if let Err(e) = client.send_packets() {
            renet_error.send(e);
        }
    }

    pub fn with_default_system_setup(mut self, default_system_setup: bool) -> Self {
        self.default_system_setup = default_system_setup;
        self
    }

    pub fn get_systems(label: RenetSystems) -> SystemSet {
        match label {
            RenetSystems::ClearEvents => SystemSet::new()
                .with_system(Events::<RenetError>::update_system)
                .label(RenetSystems::ClearEvents),
            RenetSystems::Update => SystemSet::new()
                .with_system(Self::update_system.with_run_criteria(has_resource::<RenetClient>))
                .label(RenetSystems::Update),
            RenetSystems::SendPackets => SystemSet::new().with_system(
                Self::send_packets
                    .with_run_criteria(has_resource::<RenetClient>)
                    .label(RenetSystems::SendPackets),
            ),
        }
    }
}

fn has_resource<T: Resource>(resource: Option<Res<T>>) -> ShouldRun {
    match resource.is_some() {
        true => ShouldRun::Yes,
        false => ShouldRun::No,
    }
}

pub fn run_if_client_connected(client: Option<Res<RenetClient>>) -> ShouldRun {
    match client {
        Some(client) if client.is_connected() => ShouldRun::Yes,
        _ => ShouldRun::No,
    }
}

#[cfg(test)]
mod tests {
    use crate::renet::{ClientAuthentication, RenetConnectionConfig, ServerAuthentication, ServerConfig};
    use std::{
        error::Error,
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        time::SystemTime,
    };

    use super::*;

    #[test]
    fn sending_and_receiving_messages() {
        let server = create_server().unwrap();
        let client = create_client().unwrap();

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugin(RenetServerPlugin::default())
            .add_plugin(RenetClientPlugin::default())
            .insert_resource(server)
            .insert_resource(client);

        app.update();
        app.update();
        app.update();

        let client = app.world.resource::<RenetClient>();
        assert!(client.is_connected(), "The client should be connected to the server",);

        let client_id = client.client_id();
        for index in 0..10 {
            // Send message from server to client
            let server_message = format!("Hello from server {}", index).as_bytes().to_vec();
            let mut server = app.world.resource_mut::<RenetServer>();
            server.send_message(client_id, 0, server_message.clone());

            app.update();
            app.update();

            let mut client = app.world.resource_mut::<RenetClient>();
            let message = client.receive_message(0).expect("Unable to receive message from server");
            assert_eq!(message, server_message);

            // Send message from client to server
            let client_message = format!("Hello from client {}", index).as_bytes().to_vec();
            client.send_message(0, client_message.clone());

            app.update();
            app.update();

            let mut server = app.world.resource_mut::<RenetServer>();
            let message = server.receive_message(client_id, 0).expect("Unable to receive message from client");
            assert_eq!(message, client_message);
        }
    }

    const SERVER_PORT: u16 = 4444;
    const SERVER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    const PROTOCOL_ID: u64 = 7;

    fn create_server() -> Result<RenetServer, Box<dyn Error>> {
        let server_addr = SocketAddr::new(SERVER_IP, SERVER_PORT);
        RenetServer::new(
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?,
            ServerConfig::new(64, PROTOCOL_ID, server_addr, ServerAuthentication::Unsecure),
            RenetConnectionConfig::default(),
            UdpSocket::bind(server_addr)?,
        )
        .map_err(From::from)
    }

    fn create_client() -> Result<RenetClient, Box<dyn Error>> {
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let client_id = current_time.as_millis() as u64;
        let ip = SERVER_IP;
        let authentication = ClientAuthentication::Unsecure {
            client_id,
            server_addr: SocketAddr::new(ip, SERVER_PORT),
            protocol_id: PROTOCOL_ID,
            user_data: None,
        };
        RenetClient::new(
            current_time,
            UdpSocket::bind((ip, 0))?,
            RenetConnectionConfig::default(),
            authentication,
        )
        .map_err(From::from)
    }
}
