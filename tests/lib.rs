use std::{collections::HashMap, net::{SocketAddr, ToSocketAddrs, UdpSocket}};

use renet::{
    channel::{ChannelConfig, ReliableOrderedChannelConfig, UnreliableUnorderedChannelConfig},
    client::{Client, RemoteClientConnected, RequestConnection},
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol, UnsecureService},
    server::Server,
};

use bincode;
use serde::{Deserialize, Serialize};

enum Channels {
    Reliable,
    Unreliable,
}

impl Into<u8> for Channels {
    fn into(self) -> u8 {
        match self {
            Channels::Reliable => 0,
            Channels::Unreliable => 1,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TestMessage {
    value: u64,
}

fn channels_config() -> HashMap<u8, Box<dyn ChannelConfig>> {
    let reliable_config = ReliableOrderedChannelConfig::default();
    let unreliable_config = UnreliableUnorderedChannelConfig::default();

    let mut channels_config: HashMap<u8, Box<dyn ChannelConfig>> = HashMap::new();
    channels_config.insert(Channels::Reliable.into(), Box::new(reliable_config));
    channels_config.insert(Channels::Unreliable.into(), Box::new(unreliable_config));
    channels_config
}

fn setup_server<A: ToSocketAddrs>(addr: A) -> Server<UnsecureServerProtocol> {
    let socket = UdpSocket::bind(addr).unwrap();

    let server: Server<UnsecureServerProtocol> = Server::new(
        socket,
        Default::default(),
        Default::default(),
        channels_config(),
    )
    .unwrap();

    server
}

fn request_remote_connection<A: ToSocketAddrs>(
    id: u64,
    addr: A,
    server_addr: SocketAddr,
) -> RequestConnection<UnsecureClientProtocol> {
    let socket = UdpSocket::bind(addr).unwrap();
    let request_connection = RequestConnection::new(
        id,
        socket,
        server_addr,
        UnsecureClientProtocol::new(id),
        Default::default(),
        channels_config(),
    )
    .unwrap();

    request_connection
}

fn connect_to_server(
    server: &mut Server<UnsecureServerProtocol>,
    mut request: RequestConnection<UnsecureClientProtocol>,
) -> RemoteClientConnected<UnsecureService> {
    // TODO: setup max iterations to try to connect
    loop {
        match request.update() {
            Ok(Some(connection)) => {
                return connection;
            }
            Ok(None) => {}
            Err(_) => {
                panic!("Failed to connect!");
            }
        }

        server.update();
        server.send_packets();
    }
}

#[test]
fn test_remote_connection_reliable_channel() {
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let mut server = setup_server("127.0.0.1:5000");
    let request_connection = request_remote_connection(0, "127.0.0.1:6000", server_addr);
    let mut remote_connection = connect_to_server(&mut server, request_connection);

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::serialize(&message).unwrap();
        server.send_message_to_all_clients(Channels::Reliable, message.into_boxed_slice());
    }

    server.update();
    server.send_packets();
    
    loop {
        remote_connection.process_events().unwrap();
        while let Some(message) = remote_connection.receive_message(Channels::Reliable.into()) {
            let message: TestMessage = bincode::deserialize(&message).unwrap();
            assert_eq!(current_message_number, message.value);
            current_message_number += 1;
        }

        if current_message_number == number_messages {
            break;
        }
    }

    assert_eq!(number_messages, current_message_number);
}

#[test]
fn test_remote_connection_unreliable_channel() {
    let server_addr = "127.0.0.1:5001".parse().unwrap();
    let mut server = setup_server(server_addr);
    let request_connection = request_remote_connection(0, "127.0.0.1:6001", server_addr);
    let mut remote_connection = connect_to_server(&mut server, request_connection);

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::serialize(&message).unwrap();
        server.send_message_to_all_clients(Channels::Unreliable, message.into_boxed_slice());
    }

    server.update();
    server.send_packets();
    
    loop {
        remote_connection.process_events().unwrap();
        while let Some(message) = remote_connection.receive_message(Channels::Unreliable.into()) {
            let message: TestMessage = bincode::deserialize(&message).unwrap();
            assert_eq!(current_message_number, message.value);
            current_message_number += 1;
        }

        if current_message_number == number_messages {
            break;
        }
    }

    assert_eq!(number_messages, current_message_number);
}
