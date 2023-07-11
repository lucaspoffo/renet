use std::{net::UdpSocket, time::SystemTime};

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};
use bevy_egui::EguiPlugin;
use bevy_rapier3d::prelude::*;
use bevy_renet::{
    renet::{
        transport::{NetcodeServerTransport, ServerAuthentication, ServerConfig},
        RenetServer,
    },
    transport::NetcodeServerPlugin,
    RenetServerPlugin,
};
use demo_bevy::{connection_config, server::*, setup_level, PROTOCOL_ID};
use renet_visualizer::RenetServerVisualizer;

fn new_renet_server() -> (RenetServer, NetcodeServerTransport) {
    let server = RenetServer::new(connection_config());

    let public_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(public_addr).unwrap();
    let server_config = ServerConfig {
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        public_addr,
        authentication: ServerAuthentication::Unsecure,
    };
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();

    let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

    (server, transport)
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);

    app.add_plugin(RenetServerPlugin);
    app.add_plugin(NetcodeServerPlugin);
    app.add_plugin(RapierPhysicsPlugin::<NoUserData>::default());
    app.add_plugin(RapierDebugRenderPlugin::default());
    app.add_plugin(FrameTimeDiagnosticsPlugin::default());
    app.add_plugin(LogDiagnosticsPlugin::default());
    app.add_plugin(EguiPlugin);

    app.insert_resource(ServerLobby::default());
    app.insert_resource(BotId(0));

    let (server, transport) = new_renet_server();
    app.insert_resource(server);
    app.insert_resource(transport);

    app.insert_resource(RenetServerVisualizer::<200>::default());

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
