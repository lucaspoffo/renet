use bevy::app::AppExit;
use bevy::prelude::*;
use renet::{RenetClient, RenetServer};
use renet_channel::{ChannelClientTransport, ChannelServerTransport};

use crate::{RenetClientPlugin, RenetServerPlugin};

pub struct ChannelServerPlugin;

pub struct ChannelClientPlugin;

impl Plugin for ChannelServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<RenetServer>())
                .run_if(resource_exists::<ChannelServerTransport>())
                .after(RenetServerPlugin::update_system),
        )
        .add_systems(
            PostUpdate,
            (Self::send_packets, Self::disconnect_on_exit)
                .run_if(resource_exists::<RenetServer>())
                .run_if(resource_exists::<ChannelServerTransport>()),
        );
    }
}

impl ChannelServerPlugin {
    pub fn update_system(mut transport: ResMut<ChannelServerTransport>, mut server: ResMut<RenetServer>) {
        transport.update(&mut server);
    }

    pub fn send_packets(mut transport: ResMut<ChannelServerTransport>, mut server: ResMut<RenetServer>) {
        transport.send_packets(&mut server);
    }

    fn disconnect_on_exit(exit: EventReader<AppExit>, mut transport: ResMut<ChannelServerTransport>, mut server: ResMut<RenetServer>) {
        if !exit.is_empty() {
            transport.disconnect_all(&mut server);
        }
    }
}

impl Plugin for ChannelClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            Self::update_system
                .run_if(resource_exists::<RenetClient>())
                .run_if(resource_exists::<ChannelClientTransport>())
                .after(RenetClientPlugin::update_system),
        )
        .add_systems(
            PostUpdate,
            (Self::send_packets, Self::disconnect_on_exit)
                .run_if(resource_exists::<RenetClient>())
                .run_if(resource_exists::<ChannelClientTransport>()),
        );
    }
}

impl ChannelClientPlugin {
    pub fn update_system(mut transport: ResMut<ChannelClientTransport>, mut client: ResMut<RenetClient>) {
        transport.update(&mut client);
    }

    pub fn send_packets(mut transport: ResMut<ChannelClientTransport>, mut client: ResMut<RenetClient>) {
        transport.send_packets(&mut client);
    }

    fn disconnect_on_exit(mut transport: ResMut<ChannelClientTransport>, mut client: ResMut<RenetClient>, exit: EventReader<AppExit>) {
        if !exit.is_empty() {
            transport.disconnect(&mut client);
        }
    }
}

pub fn client_connected() -> impl FnMut(Option<Res<ChannelClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => transport.is_connected(),
        None => false,
    }
}

pub fn client_disconnected() -> impl FnMut(Option<Res<ChannelClientTransport>>) -> bool {
    |transport| match transport {
        Some(transport) => !transport.is_connected(),
        None => true,
    }
}

pub fn client_connecting() -> impl FnMut(Option<Res<ChannelClientTransport>>) -> bool {
    |_transport| false
}

pub fn client_just_connected() -> impl FnMut(Local<bool>, Option<Res<ChannelClientTransport>>) -> bool {
    |mut last_connected: Local<bool>, transport| {
        let connected = transport.map(|transport| transport.is_connected()).unwrap_or(false);

        let just_connected = !*last_connected && connected;
        *last_connected = connected;
        just_connected
    }
}

pub fn client_just_disconnected() -> impl FnMut(Local<bool>, Option<Res<ChannelClientTransport>>) -> bool {
    |mut last_connected: Local<bool>, transport| {
        let disconnected = transport.map(|transport| transport.is_connected()).unwrap_or(true);

        let just_disconnected = *last_connected && disconnected;
        *last_connected = !disconnected;
        just_disconnected
    }
}

#[cfg(test)]
mod tests {
    use crate::renet::{ClientId, ConnectionConfig, DefaultChannel};

    use super::*;

    #[derive(Debug, Default, Resource, PartialEq, Eq, Deref, DerefMut)]
    pub struct ServerReceived(Vec<(u64, Vec<u8>)>);

    #[derive(Debug, Default, Resource, PartialEq, Eq, Deref, DerefMut)]
    pub struct ClientReceived(Vec<Vec<u8>>);

    fn client_received(client: &App) -> Vec<Vec<u8>> {
        client.world.resource::<ClientReceived>().0.clone()
    }

    fn server_received(server: &App) -> Vec<(u64, Vec<u8>)> {
        let mut received = server.world.resource::<ServerReceived>().0.clone();
        received.sort_by_key(|&(client_id, _)| client_id);
        received
    }

    fn create_server_app() -> App {
        let server_transport = ChannelServerTransport::default();
        let renet_server = RenetServer::new(ConnectionConfig::default());

        let mut server = App::new();
        server
            .add_plugins((MinimalPlugins, RenetServerPlugin, ChannelServerPlugin))
            .insert_resource(renet_server)
            .insert_resource(server_transport)
            .init_resource::<ServerReceived>()
            .add_systems(Update, |mut server: ResMut<RenetServer>, mut received: ResMut<ServerReceived>| {
                for client_id in server.clients_id() {
                    while let Some(packet) = server.receive_message(client_id, DefaultChannel::ReliableOrdered) {
                        received.push((client_id.raw(), packet.to_vec()));
                    }
                }
            });

        server
    }

    fn create_client_app(server: &mut App) -> App {
        let mut server_transport = server.world.resource_mut::<ChannelServerTransport>();
        let client_transport = server_transport.create_client();
        let renet_client = RenetClient::new(ConnectionConfig::default());

        let mut client = App::new();
        client
            .add_plugins((MinimalPlugins, RenetClientPlugin, ChannelClientPlugin))
            .insert_resource(renet_client)
            .insert_resource(client_transport)
            .init_resource::<ClientReceived>()
            .add_systems(Update, |mut client: ResMut<RenetClient>, mut received: ResMut<ClientReceived>| {
                while let Some(packet) = client.receive_message(DefaultChannel::ReliableOrdered) {
                    received.push(packet.to_vec());
                }
            });

        client
    }

    #[test]
    fn simple_transport() {
        let mut server = create_server_app();
        let mut client = create_client_app(&mut server);

        assert!(client.world.resource::<ChannelClientTransport>().is_connected());

        server.add_systems(Update, |mut server: ResMut<RenetServer>| {
            server.broadcast_message(DefaultChannel::ReliableOrdered, vec![1]);
        });
        server.update();
        client.update();

        assert_eq!(client_received(&client), [[1]]);
        assert_eq!(server_received(&server), []);
    }

    #[test]
    fn multiple_messages() {
        let mut server = create_server_app();
        let mut client = create_client_app(&mut server);

        server.add_systems(Update, |mut server: ResMut<RenetServer>| {
            server.broadcast_message(DefaultChannel::ReliableOrdered, vec![1, 2]);
            server.broadcast_message(DefaultChannel::ReliableOrdered, vec![3]);
        });
        server.update();
        server.update();
        client.update();

        assert_eq!(client_received(&client), [vec![1, 2], vec![3], vec![1, 2], vec![3]]);
        assert_eq!(server_received(&server), []);

        server.update();
        client.update();

        assert_eq!(
            client_received(&client),
            [vec![1, 2], vec![3], vec![1, 2], vec![3], vec![1, 2], vec![3]]
        );
        assert_eq!(server_received(&server), []);
    }

    #[test]
    fn both_directions() {
        let mut server = create_server_app();
        let mut client = create_client_app(&mut server);

        server.add_systems(Update, |mut server: ResMut<RenetServer>| {
            server.broadcast_message(DefaultChannel::ReliableOrdered, vec![1]);
        });
        client.add_systems(Update, |mut client: ResMut<RenetClient>| {
            client.send_message(DefaultChannel::ReliableOrdered, vec![2]);
        });
        server.update();
        client.update();

        assert_eq!(client_received(&client), [[1]]);
        assert_eq!(server_received(&server), []);

        server.update();

        assert_eq!(client_received(&client), [[1]]);
        assert_eq!(server_received(&server), [(0, vec![2])]);
    }

    #[test]
    fn multiple_clients() {
        let mut server = create_server_app();
        let mut client1 = create_client_app(&mut server);
        let mut client2 = create_client_app(&mut server);
        let mut client3 = create_client_app(&mut server);

        server.add_systems(Update, |mut server: ResMut<RenetServer>| {
            server.broadcast_message(DefaultChannel::ReliableOrdered, vec![0]);
        });
        client1.add_systems(Update, |mut client: ResMut<RenetClient>| {
            client.send_message(DefaultChannel::ReliableOrdered, vec![1]);
        });
        client2.add_systems(Update, |mut client: ResMut<RenetClient>| {
            client.send_message(DefaultChannel::ReliableOrdered, vec![2]);
        });
        client3.add_systems(Update, |mut client: ResMut<RenetClient>| {
            client.send_message(DefaultChannel::ReliableOrdered, vec![3]);
        });

        server.update();
        client1.update();
        client2.update();
        client3.update();

        assert_eq!(client_received(&client1), [[0]]);
        assert_eq!(client_received(&client2), [[0]]);
        assert_eq!(client_received(&client3), [[0]]);
        assert_eq!(server_received(&server), []);

        server.update();

        assert_eq!(client_received(&client1), [[0]]);
        assert_eq!(client_received(&client2), [[0]]);
        assert_eq!(client_received(&client3), [[0]]);
        assert_eq!(server_received(&server), [(0, vec![1]), (1, vec![2]), (2, vec![3])]);
    }

    #[test]
    fn disconnect_client() {
        let mut server = create_server_app();
        let mut client = create_client_app(&mut server);

        server.update();
        assert_eq!(server.world.resource::<RenetServer>().clients_id(), vec![ClientId::from_raw(0)]);
        assert!(client.world.resource::<ChannelClientTransport>().is_connected());

        client.world.send_event(AppExit);
        client.update();
        server.update();

        assert!(client.world.resource::<RenetClient>().is_disconnected());
        assert!(!client.world.resource::<ChannelClientTransport>().is_connected());

        assert!(server.world.resource::<RenetServer>().clients_id().is_empty());
    }

    #[test]
    fn disconnect_server() {
        let mut server = create_server_app();
        let mut client = create_client_app(&mut server);

        fn send_msg(mut server: ResMut<RenetServer>) {
            server.broadcast_message(DefaultChannel::ReliableOrdered, vec![0]);
        }
        server.add_systems(Update, send_msg.run_if(run_once()));

        server.update();

        assert_eq!(server.world.resource::<RenetServer>().clients_id(), [ClientId::from_raw(0)]);
        assert!(client.world.resource::<ChannelClientTransport>().is_connected());

        server.world.send_event(AppExit);
        server.update();
        client.update();

        assert!(server.world.resource::<RenetServer>().clients_id().is_empty());

        assert!(client_received(&client).is_empty());

        assert!(client.world.resource::<RenetClient>().is_disconnected());
        assert!(!client.world.resource::<ChannelClientTransport>().is_connected());
    }

    #[test]
    fn no_transport() {
        let mut server = create_server_app();
        let mut client = create_client_app(&mut server);

        server.world.remove_resource::<ChannelServerTransport>();
        client.world.remove_resource::<ChannelClientTransport>();

        server.update();
        client.update();
    }
}
