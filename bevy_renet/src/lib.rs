pub use renet;

use bevy::{
    ecs::schedule::{SystemConfig, SystemConfigs},
    prelude::*,
};

use renet::{RenetClient, RenetError, RenetServer, ServerEvent};

/// Set for networking systems.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum RenetSet {
    /// Runs when server resource available.
    Server,
    /// Runs when client resource available.
    Client,
}

pub struct RenetServerPlugin {
    /// If this option is set to false,
    /// you need to manually clear the bevy events for RenetError and ServerEvent.
    /// The systems for clearing events can be retrieved by [`RenetServerPlugin::get_clear_event_systems()`].
    /// Defaults to `true`
    pub clear_events: bool,
}

impl Default for RenetServerPlugin {
    fn default() -> Self {
        Self { clear_events: true }
    }
}

pub struct RenetClientPlugin {
    /// If this option is set to false,
    /// you need to manually clear the bevy events for RenetError.
    /// The systems for clearing events can be retrieved by [`RenetClientPlugin::get_clear_event_systems()`].
    /// Defaults to `true`
    pub clear_events: bool,
}

impl Default for RenetClientPlugin {
    fn default() -> Self {
        Self { clear_events: true }
    }
}

impl Plugin for RenetServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Events<ServerEvent>>();
        app.init_resource::<Events<RenetError>>();

        app.configure_set(RenetSet::Server.run_if(resource_exists::<RenetServer>()));

        app.add_system(Self::update_system.in_base_set(CoreSet::PreUpdate).in_set(RenetSet::Server))
            .add_system(Self::send_packets_system.in_base_set(CoreSet::PostUpdate).in_set(RenetSet::Server));

        if self.clear_events {
            app.add_systems(Self::get_clear_event_systems().in_base_set(CoreSet::PreUpdate));
        }
    }
}

impl Plugin for RenetClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Events<RenetError>>();

        app.configure_set(RenetSet::Client.run_if(resource_exists::<RenetClient>()));

        app.add_system(Self::update_system.in_base_set(CoreSet::PreUpdate).in_set(RenetSet::Client))
            .add_system(Self::send_packets_system.in_base_set(CoreSet::PostUpdate).in_set(RenetSet::Client));

        if self.clear_events {
            app.add_system(
                Self::get_clear_event_system()
                    .in_base_set(CoreSet::PreUpdate)
                    .before(Self::update_system),
            );
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

    pub fn send_packets_system(mut server: ResMut<RenetServer>, mut renet_error: EventWriter<RenetError>) {
        if let Err(e) = server.send_packets() {
            renet_error.send(RenetError::IO(e));
        }
    }

    pub fn get_clear_event_systems() -> SystemConfigs {
        (Events::<ServerEvent>::update_system, Events::<RenetError>::update_system).into_configs()
    }
}

impl RenetClientPlugin {
    pub fn update_system(mut client: ResMut<RenetClient>, mut renet_error: EventWriter<RenetError>, time: Res<Time>) {
        if let Err(e) = client.update(time.delta()) {
            renet_error.send(e);
        }
    }

    pub fn send_packets_system(mut client: ResMut<RenetClient>, mut renet_error: EventWriter<RenetError>) {
        if let Err(e) = client.send_packets() {
            renet_error.send(e);
        }
    }

    pub fn get_clear_event_system() -> SystemConfig {
        Events::<RenetError>::update_system.into_config()
    }
}

pub fn client_connecting(client: Option<Res<RenetClient>>) -> bool {
    match client {
        Some(client) => client.is_connecting(),
        None => false,
    }
}

pub fn client_connected(client: Option<Res<RenetClient>>) -> bool {
    match client {
        Some(client) => client.is_connected(),
        None => false,
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
        env_logger::init();
        let server = create_server().unwrap();
        let client = create_client().unwrap();

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugin(RenetServerPlugin::default())
            .add_plugin(RenetClientPlugin::default())
            .insert_resource(server)
            .insert_resource(client);

        // Connect client
        loop {
            app.update();
            if app.world.resource::<RenetClient>().is_connected() {
                break;
            }
        }

        let client_id = app.world.resource::<RenetClient>().client_id();
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
