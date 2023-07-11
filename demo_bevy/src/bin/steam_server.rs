use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};
use bevy_egui::EguiPlugin;
use bevy_rapier3d::prelude::*;
use bevy_renet::{
    renet::RenetServer,
    renet_steam_transport::transport::server::{SteamServerTransport, SteamTransportConfig},
    steam_transport::SteamServerPlugin,
    RenetServerPlugin,
};
use demo_bevy::{connection_config, server::*, setup_level};
use renet_visualizer::RenetServerVisualizer;
use steamworks::{Client, ClientManager, SingleClient};

#[derive(Resource, Clone)]
struct SteamClient(Client<ClientManager>);

fn new_steam_server(steam_client: &Client) -> (RenetServer, SteamServerTransport<ClientManager>) {
    let server = RenetServer::new(connection_config());
    let transport: SteamServerTransport<ClientManager> =
        SteamServerTransport::new(steam_client, SteamTransportConfig::from_max_clients(10)).unwrap();
    (server, transport)
}

fn main() {
    let (steam_client, single) = match Client::init_app(480) {
        Ok(result) => result,
        Err(err) => panic!("Failed to initialize Steam client: {}", err),
    };
    steam_client.networking_utils().init_relay_network_access();
    // wait for relay network to be available
    for _ in 0..5 {
        single.run_callbacks();
        std::thread::sleep(::std::time::Duration::from_millis(50));
    }
    let mut app = App::new();

    app.add_plugins(DefaultPlugins);
    app.add_plugin(RenetServerPlugin);
    app.add_plugin(SteamServerPlugin);

    app.add_plugin(RapierPhysicsPlugin::<NoUserData>::default());
    app.add_plugin(RapierDebugRenderPlugin::default());
    app.add_plugin(FrameTimeDiagnosticsPlugin::default());
    app.add_plugin(LogDiagnosticsPlugin::default());
    app.add_plugin(EguiPlugin);
    let (server, transport) = new_steam_server(&steam_client);
    app.insert_non_send_resource(single);
    app.insert_resource(SteamClient(steam_client));
    app.insert_resource(server);
    app.insert_resource(transport);
    app.insert_resource(ServerLobby::default());
    app.insert_resource(BotId(0));

    app.insert_resource(RenetServerVisualizer::<200>::default());

    app.add_system(steam_callbacks.in_base_set(CoreSet::First));

    app.add_systems((
        server_update_system,
        server_network_sync,
        move_players_system,
        update_projectiles_system,
        update_visulizer_system,
        despawn_projectile_system,
        spawn_bot,
        bot_autocast,
    ));

    app.add_system(projectile_on_removal_system.in_base_set(CoreSet::PostUpdate));

    app.add_startup_systems((setup_level, setup_simple_camera));

    app.run();
}

fn steam_callbacks(client: NonSend<SingleClient>) {
    client.run_callbacks();
}
