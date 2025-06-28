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

// Sets up the server to accept connections via its steam id or through the localhost address. Both methods will
// require the user to have a valid steam id when connecting. The localhost address is useful when debugging, since steam
// will reject p2p (steam id) connections of the same steam id going to the same computer.
let socket_options = SteamServerSocketOptions::new_p2p().with_address("127.0.0.1:5000".parse().unwrap());
let mut steam_transport = SteamServerTransport::new(&steam_client, steam_transport_config, socket_options).unwrap();

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
// Connect to the server via its steam id. The server should have been setup with `SteamServerSocketOptions::new_p2p`
let mut steam_transport = SteamClientTransport::new_p2p(&steam_client, &server_steam_id).unwrap();
// Alternatively, you can connect to the IP of the server directly, if the server was setup with `SteamServerSocketOptions::new_ip`.
let mut steam_transport = SteamClientTransport::new_ip(&steam_client, "127.0.0.1:5000".parse().unwrap()).unwrap();

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

You can try the steam echo example with (steam needs to be running in the background):

- server: `cargo run --example echo server`
- client: `cargo run --example echo client [HOST_STEAM_ID]`

The HOST_STEAM_ID is printed in the console when the server starts.

You can also run the echo example using steam lobbies.
Only steam users connected to the lobby will be able to connect to the server. In the echo example, they will first try connect to the steam lobby and later to the renet server:

- server: `cargo run --example echo server lobby`
- client: `cargo run --example echo client [HOST_STEAM_ID] [LOBBY_ID]`

The LOBBY_ID is printed in the console when the server starts.
