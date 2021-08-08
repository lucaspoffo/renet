use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renet::{
    channel::{
        BlockChannelConfig, ChannelConfig, ReliableOrderedChannelConfig,
        UnreliableUnorderedChannelConfig,
    },
    client::{Client, RemoteClient},
    error::{DisconnectionReason, RenetError},
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol},
    remote_connection::ConnectionConfig,
    server::{ Server, ServerConfig, ServerEvent},
    connection_control::ConnectionPermission,
    transport::{UdpClient, UdpServer},
};

use bincode;
use env_logger;
use serde::{Deserialize, Serialize};

pub fn init_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}

enum Channels {
    Reliable,
    Unreliable,
    Block,
}

impl Into<u8> for Channels {
    fn into(self) -> u8 {
        match self {
            Channels::Reliable => 0,
            Channels::Unreliable => 1,
            Channels::Block => 2,
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
    let block_config = BlockChannelConfig::default();

    let mut channels_config: HashMap<u8, Box<dyn ChannelConfig>> = HashMap::new();
    channels_config.insert(Channels::Reliable.into(), Box::new(reliable_config));
    channels_config.insert(Channels::Unreliable.into(), Box::new(unreliable_config));
    channels_config.insert(Channels::Block.into(), Box::new(block_config));
    channels_config
}

fn setup_server() -> Server<u64, UdpServer<u64, UnsecureServerProtocol<u64>>> {
    setup_server_with_config(Default::default(), ConnectionPermission::All)
}

fn setup_server_with_config(
    server_config: ServerConfig,
    connection_permission: ConnectionPermission,
) -> Server<u64, UdpServer<u64, UnsecureServerProtocol<u64>>> {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        heartbeat_time: Duration::from_millis(10),
        ..Default::default()
    };
    let transport = UdpServer::new(socket);
    let server: Server<u64, UdpServer<u64, UnsecureServerProtocol<u64>>> = Server::new(
        transport,
        server_config,
        connection_config,
        connection_permission,
        channels_config(),
    );

    server
}

fn request_remote_connection(
    id: u64,
    server_addr: SocketAddr,
) -> RemoteClient<u64, UdpClient<u64, UnsecureClientProtocol<u64>>> {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        heartbeat_time: Duration::from_millis(10),
        ..Default::default()
    };
    let protocol = UnsecureClientProtocol::new(id);
    let transport = UdpClient::new(server_addr, protocol, socket);
    let request_connection = RemoteClient::new(id, transport, channels_config(), connection_config);

    request_connection
}

fn connect_to_server(
    server: &mut Server<u64, UdpServer<u64, UnsecureServerProtocol<u64>>>,
    request: &mut RemoteClient<u64, UdpClient<u64, UnsecureClientProtocol<u64>>>,
) -> Result<(), RenetError> {
    loop {
        request.update()?;
        if request.is_connected() {
            println!("Request is connected");
            return Ok(());
        }
        request.send_packets().unwrap();
        server.update().unwrap();
        server.send_packets();
    }
}

#[test]
fn test_remote_connection_reliable_channel() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(0, server.connection_id());
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
fn test_remote_connection_reliable_channel_would_drop_message() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(0, server.connection_id());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = ReliableOrderedChannelConfig::default().message_send_queue_size;
    for _ in 0..number_messages {
        remote_connection
            .send_message(Channels::Reliable.into(), vec![0])
            .unwrap();
    }

    // No more messages can be sent or it will drop an unacked one.
    remote_connection.update().unwrap();

    // Send one more message than the channel can store, so it'll error when updated.
    remote_connection
        .send_message(Channels::Reliable.into(), vec![0])
        .unwrap();

    let error = remote_connection.update().unwrap_err();
    assert!(matches!(
        error,
        RenetError::ConnectionError(DisconnectionReason::ChannelError { channel_id: 0 })
    ));
}

#[test]
fn test_remote_connection_unreliable_channel() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(0, server.connection_id());
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
fn test_remote_connection_block_channel() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(0, server.connection_id());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let payload = vec![7u8; 20480];
    server
        .send_message(0, Channels::Block, payload.clone())
        .unwrap();

    let received_payload = loop {
        remote_connection.update().unwrap();
        if let Ok(Some(message)) = remote_connection.receive_message(Channels::Block.into()) {
            break message;
        }

        remote_connection.send_packets().unwrap();
        server.update().unwrap();
        server.send_packets();
    };

    assert!(
        payload == received_payload,
        "block message payload is not the same"
    );
}

#[test]
fn test_max_players_connected() {
    init_log();
    let server_config = ServerConfig {
        max_clients: 0,
        ..Default::default()
    };
    let mut server = setup_server_with_config(server_config, ConnectionPermission::All);
    let mut remote_connection = request_remote_connection(0, server.connection_id());
    let error = connect_to_server(&mut server, &mut remote_connection).unwrap_err();
    assert!(matches!(
        error,
        RenetError::ConnectionError(DisconnectionReason::MaxPlayer)
    ));
}

#[test]
fn test_server_disconnect_client() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(0, server.connection_id());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    server.disconnect(&0);
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
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(0, server.connection_id());
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
    let mut server = setup_server();
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

#[test]
fn test_connection_permission_none() {
    init_log();
    let mut server = setup_server_with_config(Default::default(), ConnectionPermission::None);
    let mut remote_connection = request_remote_connection(0, server.connection_id());
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::Denied))
    ));
}

#[test]
fn test_connection_permission_only_allowed() {
    init_log();
    let mut server =
        setup_server_with_config(Default::default(), ConnectionPermission::OnlyAllowed);
    let client_id = 0;
    let mut remote_connection =
        request_remote_connection(client_id.clone(), server.connection_id());
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::Denied))
    ));

    let mut remote_connection =
        request_remote_connection(client_id.clone(), server.connection_id());
    server.allow_client(&client_id);
    connect_to_server(&mut server, &mut remote_connection).unwrap();
}

#[test]
fn test_connection_permission_denied() {
    init_log();
    let mut server = setup_server_with_config(Default::default(), ConnectionPermission::All);
    let client_id = 0;
    server.deny_client(&client_id);
    let mut remote_connection =
        request_remote_connection(client_id.clone(), server.connection_id());
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::Denied))
    ));
}
