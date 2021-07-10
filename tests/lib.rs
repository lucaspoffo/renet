use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    time::Duration,
};

use renet::{
    channel::{ChannelConfig, ReliableOrderedChannelConfig, UnreliableUnorderedChannelConfig},
    client::{Client, RemoteClient},
    error::ConnectionError,
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol},
    remote_connection::ConnectionConfig,
    server::{Server, ServerConfig},
    RenetError,
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
    setup_server_with_config(addr, Default::default())
}

fn setup_server_with_config<A: ToSocketAddrs>(
    addr: A,
    server_config: ServerConfig,
) -> Server<UnsecureServerProtocol> {
    let socket = UdpSocket::bind(addr).unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        ..Default::default()
    };
    let server: Server<UnsecureServerProtocol> =
        Server::new(socket, server_config, connection_config, channels_config()).unwrap();

    server
}

fn request_remote_connection<A: ToSocketAddrs>(
    id: u64,
    addr: A,
    server_addr: SocketAddr,
) -> RemoteClient<UnsecureClientProtocol> {
    let socket = UdpSocket::bind(addr).unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        ..Default::default()
    };
    let request_connection = RemoteClient::new(
        id,
        socket,
        server_addr,
        channels_config(),
        UnsecureClientProtocol::new(id),
        connection_config,
    )
    .unwrap();

    request_connection
}

fn connect_to_server(
    server: &mut Server<UnsecureServerProtocol>,
    request: &mut RemoteClient<UnsecureClientProtocol>,
) -> Result<(), RenetError> {
    loop {
        request.update()?;
        if request.is_connected() {
            return Ok(());
        }
        request.send_packets()?;
        server.update()?;
        server.send_packets();
    }
}

#[test]
fn test_remote_connection_reliable_channel() {
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let mut server = setup_server(server_addr);
    let mut remote_connection = request_remote_connection(0, "127.0.0.1:6000", server_addr);
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::serialize(&message).unwrap();
        server.broadcast_message(Channels::Reliable, message);
    }

    server.update().unwrap();
    server.send_packets();

    loop {
        remote_connection.update().unwrap();
        while let Ok(Some(message)) = remote_connection.receive_message(Channels::Reliable.into()) {
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
    let mut remote_connection = request_remote_connection(0, "127.0.0.1:6001", server_addr);
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::serialize(&message).unwrap();
        server.broadcast_message(Channels::Unreliable, message);
    }

    server.update().unwrap();
    server.send_packets();

    loop {
        remote_connection.update().unwrap();
        while let Ok(Some(message)) = remote_connection.receive_message(Channels::Unreliable.into())
        {
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
fn test_max_players_connected() {
    let server_addr = "127.0.0.1:5002".parse().unwrap();
    let server_config = ServerConfig {
        max_clients: 0,
        ..Default::default()
    };
    let mut server = setup_server_with_config(server_addr, server_config);
    let mut remote_connection = request_remote_connection(0, "127.0.0.1:6002", server_addr);
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(error.is_err());
    match error {
        Err(RenetError::ConnectionError(ConnectionError::MaxPlayer)) => {}
        _ => unreachable!("ConnectionError::MaxPlayer error not occurred."),
    }
}
