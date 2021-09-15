use std::{
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renet::{
    channel::reliable::ReliableChannelConfig,
    client::{Client, RemoteClient},
    error::{DisconnectionReason, RenetError},
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol},
    remote_connection::ConnectionConfig,
    server::{ConnectionPermission, SendTarget, Server, ServerConfig, ServerEvent},
};

use bincode::{self, Options};
use env_logger;
use serde::{Deserialize, Serialize};

pub fn init_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[derive(Debug, Serialize, Deserialize)]
struct TestMessage {
    value: u64,
}

fn reliable_channels_config() -> Vec<ReliableChannelConfig> {
    let reliable_config = ReliableChannelConfig::default();
    vec![reliable_config]
}

fn setup_server() -> Server<UnsecureServerProtocol<u64>> {
    setup_server_with_config(Default::default(), ConnectionPermission::All)
}

fn setup_server_with_config(
    server_config: ServerConfig,
    connection_permission: ConnectionPermission,
) -> Server<UnsecureServerProtocol<u64>> {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        heartbeat_time: Duration::from_millis(10),
        ..Default::default()
    };
    let server: Server<UnsecureServerProtocol<u64>> = Server::new(
        socket,
        server_config,
        connection_config,
        connection_permission,
        reliable_channels_config(),
    )
    .unwrap();

    server
}

fn request_remote_connection(
    id: u64,
    server_addr: SocketAddr,
) -> RemoteClient<UnsecureClientProtocol<u64>> {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        heartbeat_time: Duration::from_millis(10),
        ..Default::default()
    };
    let request_connection = RemoteClient::new(
        id,
        socket,
        server_addr,
        UnsecureClientProtocol::new(id),
        connection_config,
        reliable_channels_config(),
    )
    .unwrap();

    request_connection
}

fn connect_to_server(
    server: &mut Server<UnsecureServerProtocol<u64>>,
    request: &mut RemoteClient<UnsecureClientProtocol<u64>>,
) -> Result<(), RenetError> {
    loop {
        request.update()?;
        if request.is_connected() {
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
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::options().serialize(&message).unwrap();
        server.send_reliable_message(SendTarget::All, 0, message);
    }

    server.update().unwrap();
    server.send_packets();

    loop {
        remote_connection.update().unwrap();
        while let Some(message) = remote_connection.receive_message() {
            let message: TestMessage = bincode::options().deserialize(&message).unwrap();
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
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = ReliableChannelConfig::default().message_queue_size;
    for _ in 0..number_messages {
        remote_connection.send_reliable_message(0, vec![0]);
    }

    // No more messages can be sent or it will drop an unacked one.
    remote_connection.update().unwrap();

    // Send one more message than the channel can store, so it'll error when updated.
    remote_connection.send_reliable_message(0, vec![0]);

    let error = remote_connection.update().unwrap_err();
    assert!(matches!(
        error,
        RenetError::ConnectionError(DisconnectionReason::ReliableChannelOutOfSync {
            channel_id: 0
        })
    ));
}

#[test]
fn test_remote_connection_unreliable_channel() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::options().serialize(&message).unwrap();
        server.send_unreliable_message(SendTarget::All, message);
    }

    server.update().unwrap();
    server.send_packets();

    loop {
        remote_connection.update().unwrap();
        while let Some(message) = remote_connection.receive_message() {
            let message: TestMessage = bincode::options().deserialize(&message).unwrap();
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
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let payload = vec![7u8; 20480];
    server.send_block_message(SendTarget::All, payload.clone());

    let received_payload = loop {
        remote_connection.update().unwrap();
        if let Some(message) = remote_connection.receive_message() {
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
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
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
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
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
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
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
    let mut remote_connection = request_remote_connection(0, server.addr().unwrap());
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
        request_remote_connection(client_id.clone(), server.addr().unwrap());
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::Denied))
    ));

    let mut remote_connection =
        request_remote_connection(client_id.clone(), server.addr().unwrap());
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
        request_remote_connection(client_id.clone(), server.addr().unwrap());
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::Denied))
    ));
}
