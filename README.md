# Renet
[![Latest version](https://img.shields.io/crates/v/renet.svg)](https://crates.io/crates/renet)
[![Documentation](https://docs.rs/renet/badge.svg)](https://docs.rs/renet)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

Renet is a network library for Server/Client games written in rust. Built on top of UDP,
it is focused on fast-paced games such as FPS, and competitive games that need authentication.
Provides the following features:

- Client/Server connection management
- Authentication and encryption, checkout [renetcode](https://github.com/lucaspoffo/renet/tree/master/renetcode)
- Multiple types of channels:
    - Reliable: garantee delivery of all messages
    - Unreliable: messages that don't require any garantee of delivery or ordering
    - Chunk Reliable: slice big messages to be sent in multiple frames (e.g. level initialization)
- Packet fragmention and reassembly

Sections:
* [Usage](#usage)
* [Demos](#demos)
* [Plugins](#plugins)
* [Visualizer](#visualizer)

## Usage
Renet aims to have a simple API that is easy to integrate with any code base. Pool for new messages at the start of a frame with `update`, messages sent during a frame - or that need to be resent - are aggregated and sent together with `sent_packets`.

#### Server
```rust
let delta_time = Duration::from_millis(16);
let mut server = RenetServer::new(...);
let channel_id = 0;

// Your gameplay loop
loop {
    // Receive new messages and update clients
    server.update(delta_time)?;
    
    // Check for client connections/disconnections
    while let Some(event) = server.get_event() {
        match event {
            ServerEvent::ClientConnected(id, user_data) => {
                println!("Client {} connected", id);
            }
            ServerEvent::ClientDisconnected(id) => {
                println!("Client {} disconnected", id);
            }
        }
    }

    // Receive message from channel
    for client_id in server.clients_id().into_iter() {
        while let Some(message) = server.receive_message(client_id, channel_id) {
            // Handle received message
        }
    }
    
    // Send a text message for all clients
    server.broadcast_message(channel_id, "server message".as_bytes().to_vec());
    
    // Send message to only one client
    let client_id = ...;
    server.send_message(client_id, channel_id, "server message".as_bytes().to_vec());
 
    // Send packets to clients
    server.send_packets()?;
}
```

#### Client

```rust
let delta_time = Duration::from_millis(16);
let mut client = RenetClient::new(...);
let channel_id = 0;

// Your gameplay loop
loop {
    // Receive new messages and update client
    client.update(delta_time)?;
    
    if client.is_connected() {
        // Receive message from server
        while let Some(message) = client.receive_message(channel_id) {
            // Handle received message
        }
        
        // Send message
        client.send_message(channel_id, "client text".as_bytes().to_vec());
    }
 
    // Send packets to server
    client.send_packets()?;
}
```

## Demos
You can checkout the [echo example](https://github.com/lucaspoffo/renet/blob/master/renet/examples/echo.rs) for a simple usage of the library. Or you can look into the two demos that have more complex uses of renet:

<details><summary>Bevy Demo</summary>
<br/>
Simple bevy application to demonstrate how you could replicate entities and send reliable messages as commands from the server/client using renet:
<br/>
<br/>

[Bevy Demo.webm](https://user-images.githubusercontent.com/35241085/180664609-f8c969e0-d313-45c0-9c04-8a116896d0bd.webm)

[Repository](https://github.com/lucaspoffo/renet/tree/master/demo_bevy)
</details>

<details><summary>Chat Demo</summary>
<br/>
Simple chat application made with egui to demonstrate how you could handle errors, states transitions and client self hosting:
<br/>
<br/>

[Chat Demo.webm](https://user-images.githubusercontent.com/35241085/180664911-0baf7b35-c9d4-43ff-b793-5955060adebc.webm)

[Repository](https://github.com/lucaspoffo/renet/tree/master/demo_chat)
</details>

## Plugins

Checkout [bevy_renet](https://github.com/lucaspoffo/renet/tree/master/bevy_renet) if you want to use renet as a plugin with the [Bevy engine](https://bevyengine.org/).

## Visualizer

Checkout [renet_visualizer](https://github.com/lucaspoffo/renet/tree/master/renet_visualizer) for a egui plugin to plot metrics data from renet clients and servers:

https://user-images.githubusercontent.com/35241085/175834010-b1eafd77-7ea2-47dc-a915-a399099c7a99.mp4
