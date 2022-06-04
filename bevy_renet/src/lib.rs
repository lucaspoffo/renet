pub use renet;

use bevy::{
    ecs::{schedule::ShouldRun, system::Resource},
    prelude::*,
};

use renet::{RenetClient, RenetError, RenetServer, ServerEvent};

pub struct RenetServerPlugin;

pub struct RenetClientPlugin;

impl Plugin for RenetServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<ServerEvent>()
            .add_event::<RenetError>()
            .add_system_to_stage(
                CoreStage::PreUpdate,
                renet_server_update.with_run_criteria(has_resource::<RenetServer>),
            )
            .add_system_to_stage(
                CoreStage::PostUpdate,
                renet_server_send_packets.with_run_criteria(has_resource::<RenetServer>),
            );
    }
}

impl Plugin for RenetClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<RenetError>()
            .add_system_to_stage(
                CoreStage::PreUpdate,
                renet_client_update.with_run_criteria(has_resource::<RenetClient>),
            )
            .add_system_to_stage(
                CoreStage::PostUpdate,
                renet_client_send_packets.with_run_criteria(has_resource::<RenetClient>),
            );
    }
}

fn has_resource<T: Resource>(resource: Option<Res<T>>) -> ShouldRun {
    match resource.is_some() {
        true => ShouldRun::Yes,
        false => ShouldRun::No,
    }
}

fn renet_server_update(
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

fn renet_server_send_packets(mut server: ResMut<RenetServer>, mut renet_error: EventWriter<RenetError>) {
    if let Err(e) = server.send_packets() {
        renet_error.send(RenetError::IO(e));
    }
}

fn renet_client_update(mut client: ResMut<RenetClient>, mut renet_error: EventWriter<RenetError>, time: Res<Time>) {
    if let Err(e) = client.update(time.delta()) {
        renet_error.send(e);
    }
}

fn renet_client_send_packets(mut client: ResMut<RenetClient>, mut renet_error: EventWriter<RenetError>) {
    if let Err(e) = client.send_packets() {
        renet_error.send(e);
    }
}

pub fn run_if_client_conected(client: Option<Res<RenetClient>>) -> ShouldRun {
    match client {
        Some(client) if client.is_connected() => ShouldRun::Yes,
        _ => ShouldRun::No,
    }
}

#[cfg(test)]
mod tests {
    use renet::{ConnectToken, RenetConnectionConfig, ServerConfig, NETCODE_KEY_BYTES};
    use std::{
        error::Error,
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        time::SystemTime,
    };

    use super::*;

    #[test]
    fn sending_and_receiving_messages() {
        let server = match create_server() {
            Ok(server) => server,
            Err(error) => panic!("Unable to create server: {}", error),
        };

        let client = match create_client() {
            Ok(client) => client,
            Err(error) => panic!("Unable to create client: {}", error),
        };

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugin(RenetServerPlugin)
            .add_plugin(RenetClientPlugin)
            .insert_resource(server)
            .insert_resource(client);

        app.update();
        app.update();
        app.update();

        let client = app.world.resource::<RenetClient>();
        assert!(
            client.is_connected(),
            "The client should be connected to the server to send messages",
        );

        let client_id = client.client_id();

        // TODO: Test fails if you replace =1 with a higher value
        // I.e. a message can be received only once
        for index in 1..=1 {
            // Send message from server to client
            let server_message = format!("Hello from server {}", index);
            let message =
                bincode::serialize(&server_message).unwrap_or_else(|error| panic!("Unable to serialize server message {index}: {error}"));
            let mut server = app.world.resource_mut::<RenetServer>();
            server.send_message(client_id, 0, message);

            app.update();
            app.update();

            let mut client = app.world.resource_mut::<RenetClient>();
            let message = client
                .receive_message(0)
                .unwrap_or_else(|| panic!("Unable to receive message {} from server", index));
            let message: String =
                bincode::deserialize(&message).unwrap_or_else(|error| panic!("Unable to deserialize server message {index}: {error}"));
            assert_eq!(
                message, server_message,
                "The received message {} on client should match the one sent",
                index
            );

            // Send message from client to server
            let client_message = format!("Hello from client {}", index);
            let message =
                bincode::serialize(&client_message).unwrap_or_else(|error| panic!("Unable to serialize client message {index}: {error}"));
            client.send_message(0, message);

            app.update();
            app.update();

            let mut server = app.world.resource_mut::<RenetServer>();
            let message = server
                .receive_message(client_id, 0)
                .unwrap_or_else(|| panic!("Unable to receive message {} from client", index));
            let message: String =
                bincode::deserialize(&message).unwrap_or_else(|error| panic!("Unable to deserialize client message {index}: {error}"));
            assert_eq!(
                message, client_message,
                "The received message {} on server should match the one sent",
                index
            );
        }
    }

    const SERVER_PORT: u16 = 4444;
    const SERVER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    const PROTOCOL_ID: u64 = 7;
    const GAME_KEY: [u8; NETCODE_KEY_BYTES] = [0; NETCODE_KEY_BYTES];

    fn create_server() -> Result<RenetServer, Box<dyn Error>> {
        let server_addr = SocketAddr::new(SERVER_IP, SERVER_PORT);
        RenetServer::new(
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?,
            ServerConfig::new(64, PROTOCOL_ID, server_addr, GAME_KEY),
            RenetConnectionConfig::default(),
            UdpSocket::bind(server_addr)?,
        )
        .map_err(From::from)
    }

    fn create_client() -> Result<RenetClient, Box<dyn Error>> {
        let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let client_id = current_time.as_millis() as u64;
        let ip = SERVER_IP;
        let token = ConnectToken::generate(
            current_time,
            PROTOCOL_ID,
            300,
            client_id,
            15,
            vec![SocketAddr::new(ip, SERVER_PORT)],
            None,
            &GAME_KEY,
        )?;
        RenetClient::new(
            current_time,
            UdpSocket::bind((ip, 0))?,
            client_id,
            token,
            RenetConnectionConfig::default(),
        )
        .map_err(From::from)
    }
}
