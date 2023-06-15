# Bevy Renet
[![Latest version](https://img.shields.io/crates/v/bevy_renet.svg)](https://crates.io/crates/bevy_renet)
[![Documentation](https://docs.rs/bevy_renet/badge.svg)](https://docs.rs/bevy_renet)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

A Bevy Plugin for the [renet](https://github.com/lucaspoffo/renet) crate.
A network crate for Server/Client with cryptographically secure authentication and encypted packets.
Designed for fast paced competitive multiplayer games.

## Usage
Bevy renet is a small layer over the `renet` crate, it adds systems to call the update function from the client/server. `RenetClient`, `RenetServer`, `NetcodeClientTransport` and `NetcodeServerTransport` need to be added as a resource, so the setup is similar to `renet` itself:

#### Server
```rust
let mut app = App::new();
app.add_plugin(RenetServerPlugin);

let server = RenetServer::new(ConnectionConfig::default());
app.insert_resource(server);

// Transport layer setup
app.add_plugin(NetcodeServerPlugin);
let server_addr = "127.0.0.1:5000".parse().unwrap();
let socket = UdpSocket::bind(server_addr).unwrap();
const MAX_CLIENTS: usize = 64;
const GAME_PROTOCOL_ID: u64 = 0;
let server_config = ServerConfig::new(MAX_CLIENTS, PROTOCOL_ID, server_addr, ServerAuthentication::Unsecure);
let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
let transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();
app.insert_resource(transport);

app.add_system(send_message_system);
app.add_system(receive_message_system);
app.add_system(handle_events_system);

// Systems

fn send_message_system(mut server: ResMut<RenetServer>) {
    let channel_id = 0;
    // Send a text message for all clients
    // The enum DefaultChannel describe the channels used by the default configuration
    server.broadcast_message(DefaultChannel::ReliableOrdered, "server message".as_bytes().to_vec());
}

fn receive_message_system(mut server: ResMut<RenetServer>) {
     // Send a text message for all clients
    for client_id in server.clients_id().into_iter() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered) {
            // Handle received message
        }
    }
}

fn handle_events_system(mut server_events: EventReader<ServerEvent>) {
    while let Some(event) = server.get_event() {
    for event in server_events.iter() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Client {client_id} connected");
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Client {client_id} disconnected: {reason}");
            }
        }
    }
}
```

#### Client
```rust
let mut app = App::new();
app.add_plugin(RenetClientPlugin);

let client = RenetClient::new(ConnectionConfig::default());
app.insert_resource(client);

// Setup the transport layer
app.add_plugin(NetcodeClientPlugin);
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
app.insert_resource(transport);

app.add_system(send_message_system);
app.add_system(receive_message_system);

// Systems

fn send_message_system(mut client: ResMut<RenetClient>) {
     // Send a text message to the server
    client.send_message(DefaultChannel::ReliableOrdered, "server message".as_bytes().to_vec());
}

fn receive_message_system(mut client: ResMut<RenetClient>) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        // Handle received message
    }
}
```

## Example

You can run the `simple` example with:
* Server: `cargo run --example simple -- server`
* Client: `cargo run --example simple -- client`

If you want a more complex example you can checkout the [demo_bevy](https://github.com/lucaspoffo/renet/tree/master/demo_bevy) sample:

[Bevy Demo.webm](https://user-images.githubusercontent.com/35241085/180664609-f8c969e0-d313-45c0-9c04-8a116896d0bd.webm)

## Bevy Compatibility

|bevy|bevy_renet|
|---|---|
|0.10|0.0.8|
|0.9|0.0.6|
|0.8|0.0.5|
|0.7|0.0.4|

## The Bleeding Edge

Read more about Bleeding Edge Bevy in the [Unofficial Bevy Cheat Book](https://bevy-cheatbook.github.io/setup/bevy-git.html). Please note: bleeding edge Bevy and by extension bleeding edge Renet is very unstable.

There is more than one way to do this but I will be covering only one. The idea is to make your project depend on the exact same commit of Bevy that Renet depends on. This makes sure Renet and your project are on the same page and there aren't conflicting definitions. Manual updates are required but it means you can choose when to deal with breaking changes.

Open `bevy_renet/Cargo.toml` on the `bleeding-edge` branch. Copy the `bevy` dependency into your project's `Cargo.toml`. It will look something like:
```toml 
bevy = {git = "https://github.com/bevyengine/bevy?rev=a420beb0"}
```
Notice it specifies an exact commit using `?rev=` followed by a commit hash. Now copy the latest commit hash in the `bleeding-edge` branch and add a dependency for `bevy_renet` in your project's Cargo.toml. It should look something like:
```toml 
bevy_renet = {git = "https://github.com/roanv/renet?rev=146123ea"}
```

You should end up with something like:

```toml
# YOUR_PROJECT/Cargo.toml
[dependencies]
bevy = {git = "https://github.com/bevyengine/bevy?rev=a420beb0"} # matching bevy/main commit
bevy_renet = {git = "https://github.com/roanv/renet?rev=146123ea"} # latest bleeding-edge commit 
```
```toml
# bevy_renet/Cargo.toml on bleeding-edge branch for latest commit #146123ea
bevy = {git = "https://github.com/bevyengine/bevy?rev=a420beb0"} # matching bevy/main commit
```

To update, simply repeat the process with the latest commit in the `bleeding-edge` branch.

#### bUt i WaNt tHe lAteSt bEvY!

Have a look at the [Unofficial Bevy Cheat Book](https://bevy-cheatbook.github.io/setup/bevy-git.html). There is a way to patch bleeding-edge to depend on a more recent Bevy commit. This will only work if no breaking changes have been made to parts Renet uses.