# Rust Easy Networking
Collection of crates to create Server/Client networked games.

## Renet
Renet is a network Server/Client library in rust to generate packets from aggregated messages from multiple channels types. These messages can be:

- Reliable Ordered: garantee ordering and delivery of all packets, with configurable resend time;
- Unreliable Unordered: messages that don't require any garantee of delivery or ordering;
- Block Reliable: for bigger messages, but only one can be sent at a time per channel.

This crate does not dependend on any transport layer, it's supposed to be used to create an reliable and fast Server/Client network.
It does not have authentication.

## Renet Udp
Implementation of an Server/Client using UDP and Renet (does not have authentication yet).
#### Echo example
##### Server

```rust
let socket = UdpSocket::bind("127.0.0.0:5000").unwrap();
let server_config = ServerConfig::default();
// Create one channel for each type reliable (0), unreliable(1), block(2)
let connection_config = ConnectionConfig::default();
let mut server: UdpServer = UdpServer::new(server_config, connection_config, socket)?;
    
let frame_duration = Duration::from_millis(100);
loop {
  server.update(frame_duration)?;
  while let Some(event) = server.get_event() {
    match event {
      ServerEvent::ClientConnected(id) => println!("Client {} connected.", id),
      ServerEvent::ClientDisconnected(id, reason) => println!("Client {} disconnected: {}", id, reason)
    }
  }

  for client_id in server.clients_id().iter() {
    while let Some(message) = server.receive_message(client_id, 0) {
      let text = String::from_utf8(message)?;
      println!("Client {} sent text: {}", client_id, text);
      server.broadcast_message(0, text.as_bytes().to_vec());
    }
  }
        
  server.send_packets()?;
  thread::sleep(frame_duration);
}
```

##### Client
```rust
let socket = UdpSocket::bind("127.0.0.1:0")?;
// Create one channel for each type reliable (0), unreliable(1), block(2)
let connection_config = ConnectionConfig::default();
let server_addr = "127.0.0.1:5000".parse().unwrap();
let mut client = UdpClient::new(socket, server_addr, connection_config)?;
let stdin_channel = spawn_stdin_channel();

let frame_duration = Duration::from_millis(100);
loop {
  client.update(frame_duration)?;
  match stdin_channel.try_recv() {
    Ok(text) => client.send_message(0, text.as_bytes().to_vec())?,
    Err(TryRecvError::Empty) => {}
    Err(TryRecvError::Disconnected) => panic!("Channel disconnected"),
  }

  while let Some(text) = client.receive_message(0) {
    let text = String::from_utf8(text).unwrap();
    println!("Message from server: {}", text);
  }

  client.send_packets()?;
  thread::sleep(frame_duration);
}

fn spawn_stdin_channel() -> Receiver<String> {
  let (tx, rx) = mpsc::channel::<String>();
  thread::spawn(move || loop {
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer).unwrap();
    tx.send(buffer).unwrap();
  });
  rx
}

```
 
