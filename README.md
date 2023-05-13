# Renet
[![Latest version](https://img.shields.io/crates/v/renet.svg)](https://crates.io/crates/renet)
[![Documentation](https://docs.rs/renet/badge.svg)](https://docs.rs/renet)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

Renet is a network library for Server/Client games written in rust. It is focused on fast-paced games such as FPS, and competitive games.
Provides the following features:

- Client/Server connection management
- Authentication and encryption, checkout [renetcode](https://github.com/lucaspoffo/renet/tree/master/renetcode)
- Multiple types of channels:
    - Reliable: garantee delivery of all messages
    - Unreliable: messages that don't require any garantee of delivery or ordering
- Packet fragmention and reassembly
- Transport layer customization
    - Disable the default transport layer and integrate your own

Sections:
* [Usage](#usage)
* [Demos](#demos)
* [Plugins](#plugins)
* [Visualizer](#visualizer)

## Usage
Renet aims to have a simple API that is easy to integrate with any code base. Pool for new messages at the start of a frame with `update`. Call `send_packets` from the transport layer to send packets to client/server.

#### Server
```rust
let mut server = RenetServer::new(ConnectionConfig::default());

const GAME_PROTOCOL_ID: u64 = 0;
const MAX_NUM_PLAYERS: usize = 64;
const SERVER_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1), 5000));

let socket: UdpSocket = UdpSocket::bind(SERVER_ADDR).unwrap();
let server_config = ServerConfig::new(MAX_NUM_PLAYERS, PROTOCOL_ID, SERVER_ADDR,ServerAuthentication::Unsecure);
let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
let mut transport = NetcodeServerTransport::new(current_time, server_config, socket).unwrap();

let channel_id: u8 = 0;

// Your gameplay loop
loop {
    let delta_time = Duration::from_millis(16);
    // Receive new messages and update clients
    server.update(delta_time)?;
    transport.update(delta_time, &mut server)?;
    
    // Check for client connections/disconnections
    while let Some(event) = server.get_event() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Client {client_id} connected");
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Client {client_id} disconnected: {reason}");
            }
        }
    }

    // Receive message from channel
    for client_id in server.connections_id() {
        while let Some(message) = server.receive_message(client_id, channel_id) {
            // Handle received message
        }
    }
    
    // Send a text message for all clients
    server.broadcast_message(channel_id, "server message".as_bytes().to_vec());
    
    // Send message to only one client
    let client_id = 0; 
    server.send_message(client_id, channel_id, "server message".as_bytes().to_vec());
 
    // Send packets to clients
    transport.send_packets(&mut server)?;
}
```

#### Client

```rust
let mut client = RenetClient::new(ConnectionConfig::default());

const SERVER_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1), 5000));
const GAME_PROTOCOL_ID: u64 = 0;

let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
let client_id: u64 = 0;
let authentication = ClientAuthentication::Unsecure {
    server_addr: SERVER_ADDR,
    client_id,
    user_data: None,
    protocol_id: GAME_PROTOCOL_ID,
};

let mut transport = NetcodeClientTransport::new(socket, current_time, authentication).unwrap();

let channel_id = 0;

// Your gameplay loop
loop {
    let delta_time = Duration::from_millis(16);
    // Receive new messages and update client
    client.update(delta_time)?;
    transport.update(delta_time, &mut client).unwrap();
    
    if client.is_connected() {
        // Receive message from server
        while let Some(message) = client.receive_message(channel_id) {
            // Handle received message
        }
        
        // Send message
        client.send_message(channel_id, "client text".as_bytes().to_vec());
    }
 
    // Send packets to server
    transport.send_packets(&mut client)?;
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
