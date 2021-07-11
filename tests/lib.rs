use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    time::Duration,
};

use renet::{
    channel::{ChannelConfig, ReliableOrderedChannelConfig, UnreliableUnorderedChannelConfig},
    client::{Client, RemoteClient},
    error::DisconnectionReason,
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol},
    remote_connection::ConnectionConfig,
    server::{Server, ServerConfig, ServerEvent},
    RenetError,
};

use bincode;
use serde::{Deserialize, Serialize};
use env_logger;

fn init_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}

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
    init_log();
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
    init_log();
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
    init_log();
    let server_addr = "127.0.0.1:5002".parse().unwrap();
    let server_config = ServerConfig {
        max_clients: 0,
        ..Default::default()
    };
    let mut server = setup_server_with_config(server_addr, server_config);
    let mut remote_connection = request_remote_connection(0, "127.0.0.1:6002", server_addr);
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::MaxPlayer))
    ));
}

#[test]
fn test_server_disconnect_client() {
    init_log();
    let server_addr = "127.0.0.1:5003".parse().unwrap();
    let mut server = setup_server(server_addr);
    let mut remote_connection = request_remote_connection(0, "127.0.0.1:6003", server_addr);
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    server.disconnect(0);
    let connect_event = server.get_event().unwrap();
    assert!(matches!(connect_event, ServerEvent::ClientConnected(0)));
    let error = remote_connection.update();
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(
            DisconnectionReason::DisconnectedByServer
        ))
    ));
    let disconnect_event = server.get_event().unwrap();
    assert!(matches!(
        disconnect_event,
        ServerEvent::ClientDisconnected(0)
    ));
}

#[test]
fn test_remote_client_disconnect() {
    init_log();
    let server_addr = "127.0.0.1:5004".parse().unwrap();
    let mut server = setup_server(server_addr);
    let mut remote_connection = request_remote_connection(0, "127.0.0.1:6004", server_addr);
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let connect_event = server.get_event().unwrap();
    assert!(matches!(connect_event, ServerEvent::ClientConnected(0)));

    remote_connection.disconnect();
    assert!(!remote_connection.is_connected());

    server.update().unwrap();

    let disconnect_event = server.get_event().unwrap();
    assert!(matches!(
        disconnect_event,
        ServerEvent::ClientDisconnected(0)
    ));
}

#[test]
fn test_local_client_disconnect() {
    init_log();
    let server_addr: SocketAddr = "127.0.0.1:5005".parse().unwrap();
    let mut server = setup_server(server_addr);
    let mut local_client = server.create_local_client(0);

    let connect_event = server.get_event().unwrap();
    assert!(matches!(connect_event, ServerEvent::ClientConnected(0)));

    assert!(local_client.is_connected());
    local_client.disconnect();
    assert!(!local_client.is_connected());

    server.update().unwrap();

    let disconnect_event = server.get_event().unwrap();
    assert!(matches!(
        disconnect_event,
        ServerEvent::ClientDisconnected(0)
    ));
}
