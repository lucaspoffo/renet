# Renet Steam
[![Latest version](https://img.shields.io/crates/v/renet_steam.svg)](https://crates.io/crates/renet_steam)
[![Documentation](https://docs.rs/renet_steam/badge.svg)](https://docs.rs/renet_steam)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

Transport layer for the [renet](https://github.com/lucaspoffo/renet) crate using the steamworks-rs.

## Usage

This crate adds `SteamServerTransport` and `SteamClientTransport` to replace the default transport layer that comes with renet:

#### Server

```rust
// Setup steam client
let (steam_client, single) = Client::init_app(480).unwrap();
steam_client.networking_utils().init_relay_network_access();

// Create renet server
let connection_config = ConnectionConfig::default();
let mut server: RenetServer = RenetServer::new(connection_config);

// Create steam transport
let access_permission = AccessPermission::Public;
let steam_transport_config = SteamServerConfig {
    max_clients: 10,
    access_permission,
};
let mut steam_transport = SteamServerTransport::new(&steam_client, steam_transport_config).unwrap();

// Your gameplay loop
loop {
    let delta_time = Duration::from_millis(16);

    single.run_callbacks(); // Update steam callbacks
   
    server.update(delta_time);
    steam_transport.update(&mut server);

    // Handle connect/disconnect events
    while let Some(event) = server.get_event() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Client {} connected.", client_id)
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Client {} disconnected: {}", client_id, reason);
            }
        }
    }

    // Code for sending/receiving messages can go here
    // Check the examples/demos 

    steam_transport.send_packets(&mut server);
    thread::sleep(delta_time);
}
```

#### Client

```rust
// Setup steam client
let (steam_client, single) = Client::init_app(480).unwrap();
steam_client.networking_utils().init_relay_network_access();

// Create renet client
let connection_config = ConnectionConfig::default();
let mut client = RenetClient::new(connection_config);

// Create steam transport
let server_steam_id = SteamId::from_raw(0); // Here goes the steam id of the host
let mut steam_transport = SteamClientTransport::new(&steam_client, &server_steam_id).unwrap();

// Your gameplay loop
loop {
    let delta_time = Duration::from_millis(16);

    single.run_callbacks(); // Update steam callbacks
    client.update(delta_time);
    steam_transport.update(&mut client);

    // Code for sending/receiving messages can go here
    // Check the examples/demos 

    steam_transport.send_packets(&mut client).unwrap();
    thread::sleep(delta_time);
}
```

## Example

You can try the steam echo example with:

- server: `cargo run --example echo server`
- client: `cargo run --example echo client [HOST_STEAM_ID]`

The HOST_STEAM_ID is printed in the console when the server starts.

You can also run the echo example using steam lobbies.
Only steam users connected to the lobby will be able to connect to the server. In the echo example, they will first try connect to the steam lobby and later to the renet server:

- server: `cargo run --example echo server lobby`
- client: `cargo run --example echo client [HOST_STEAM_ID] [LOBBY_ID]`

The LOBBY_ID is printed in the console when the server starts.

## Bevy

If you are using bevy, you can enable the `bevy` feature and instead of using the default transport that comes in `bevy_renet` you can use `SteamServerPlugin` and `SteamClientPlugin`, the setup should be similar.

You can check the [Bevy Demo](https://github.com/lucaspoffo/renet/tree/master/demo_bevy) for how to use the default and steam transport switching between them using feature flags.
