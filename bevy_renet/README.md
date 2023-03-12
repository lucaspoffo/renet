# Bevy Renet
[![Latest version](https://img.shields.io/crates/v/bevy_renet.svg)](https://crates.io/crates/bevy_renet)
[![Documentation](https://docs.rs/bevy_renet/badge.svg)](https://docs.rs/bevy_renet)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

A Bevy Plugin for the [renet](https://github.com/lucaspoffo/renet) crate.
A network crate for Server/Client with cryptographically secure authentication and encypted packets.
Designed for fast paced competitive multiplayer games.

## Usage
Bevy renet is a small layer over the `renet` crate, it adds systems to call the client/server `update`and `send_packets`. `RenetClient` and `RenetServer` need to be added as a resource, so the setup is similar to `renet` itself:

#### Server
```rust
let mut app = App::new();
app.add_plugin(RenetServerPlugin::default());

let server = RenetServer::new(...);
app.insert_resource(server);

app.add_system(send_message_system);
app.add_system(receive_message_system);
app.add_system(handle_events_system);

// Systems

fn send_message_system(mut server: ResMut<RenetServer>) {
    let channel_id = 0;
     // Send a text message for all clients
    server.broadcast_message(channel_id, "server message".as_bytes().to_vec());
}

fn receive_message_system(mut server: ResMut<RenetServer>) {
    let channel_id = 0;
     // Send a text message for all clients
    for client_id in server.clients_id().into_iter() {
        while let Some(message) = server.receive_message(client_id, channel_id) {
            // Handle received message
        }
    }
}

fn handle_events_system(mut server_events: EventReader<ServerEvent>) {
    while let Some(event) = server.get_event() {
    for event in server_events.iter() {
        match event {
            ServerEvent::ClientConnected(id, user_data) => {
                println!("Client {} connected", id);
            }
            ServerEvent::ClientDisconnected(id) => {
                println!("Client {} disconnected", id);
            }
        }
    }
}
```

#### Client
```rust
let mut app = App::new();
app.add_plugin(RenetClientPlugin::default());

let client = RenetClient::new(...);
app.insert_resource(client);

app.add_system(send_message_system);
app.add_system(receive_message_system);

// Systems

fn send_message_system(mut client: ResMut<RenetClient>) {
    let channel_id = 0;
     // Send a text message to the server
    client.send_message(channel_id, "server message".as_bytes().to_vec());
}

fn receive_message_system(mut client: ResMut<RenetClient>) {
    let channel_id = 0;
    while let Some(message) = client.receive_message(channel_id) {
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
|0.10|0.0.7|
|0.9|0.0.6|
|0.8|0.0.5|
|0.7|0.0.4|
