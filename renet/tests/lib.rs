use std::{
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use renet::{
    channel::reliable::ReliableChannelConfig,
    client::{Client, UdpClient},
    error::{DisconnectionReason, RenetError},
    remote_connection::ConnectionConfig,
    server::{ConnectionPermission, SendTarget, ServerConfig, ServerEvent},
    udp_server::UdpServer,
};

use bincode::{self, Options};
use env_logger;
use serde::{Deserialize, Serialize};

pub fn init_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TestMessage {
    value: u64,
}

fn reliable_channels_config() -> Vec<ReliableChannelConfig> {
    let reliable_config = ReliableChannelConfig::default();
    vec![reliable_config]
}

fn setup_server() -> UdpServer {
    setup_server_with_config(Default::default(), ConnectionPermission::All)
}

fn setup_server_with_config(
    server_config: ServerConfig,
    connection_permission: ConnectionPermission,
) -> UdpServer {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        heartbeat_time: Duration::from_millis(10),
        ..Default::default()
    };
    let server = UdpServer::new(
        server_config,
        connection_config,
        connection_permission,
        reliable_channels_config(),
        socket,
    )
    .unwrap();

    server
}

fn request_remote_connection(server_addr: SocketAddr) -> UdpClient {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        heartbeat_time: Duration::from_millis(10),
        ..Default::default()
    };
    let client = UdpClient::new(
        socket,
        server_addr,
        connection_config,
        reliable_channels_config(),
    )
    .unwrap();

    client
}

fn connect_to_server(server: &mut UdpServer, request: &mut UdpClient) -> Result<(), RenetError> {
    loop {
        request.update()?;
        request.send_packets().unwrap();
        server.update();
        server.send_packets();
        if request.is_connected() {
            return Ok(());
        }
    }
}

#[test]
fn test_remote_connection_reliable_channel() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::options().serialize(&message).unwrap();
        server.send_reliable_message(SendTarget::All, 0, message);
    }

    server.update();
    server.send_packets();

    loop {
        remote_connection.update().unwrap();
        while let Some(message) = remote_connection.receive_reliable_message(0) {
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
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
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
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let number_messages = 64;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::options().serialize(&message).unwrap();
        server.send_unreliable_message(SendTarget::All, message);
    }

    server.update();
    server.send_packets();

    loop {
        remote_connection.update().unwrap();
        while let Some(message) = remote_connection.receive_reliable_message(0) {
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
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let payload = vec![7u8; 20480];
    server.send_block_message(SendTarget::All, payload.clone());

    let received_payload = loop {
        remote_connection.update().unwrap();
        if let Some(message) = remote_connection.receive_block_message() {
            break message;
        }

        remote_connection.send_packets().unwrap();
        server.update();
        server.send_packets().unwrap();
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
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
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
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    server.disconnect(&remote_connection.id());
    let connect_event = server.get_event().unwrap();
    if let ServerEvent::ClientConnected(id) = connect_event {
        assert_eq!(id, remote_connection.id());
    } else {
        unreachable!()
    }
    let error = remote_connection.update();
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(
            DisconnectionReason::DisconnectedByServer
        ))
    ));
    let disconnect_event = server.get_event().unwrap();
    if let ServerEvent::ClientDisconnected(id) = disconnect_event {
        assert_eq!(id, remote_connection.id());
    } else {
        unreachable!()
    }
}

#[test]
fn test_remote_client_disconnect() {
    init_log();
    let mut server = setup_server();
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    connect_to_server(&mut server, &mut remote_connection).unwrap();

    let connect_event = server.get_event().unwrap();
    if let ServerEvent::ClientConnected(id) = connect_event {
        assert_eq!(id, remote_connection.id());
    } else {
        unreachable!()
    }

    remote_connection.disconnect();
    assert!(!remote_connection.is_connected());

    server.update();

    let disconnect_event = server.get_event().unwrap();
    if let ServerEvent::ClientDisconnected(id) = disconnect_event {
        assert_eq!(id, remote_connection.id());
    } else {
        unreachable!()
    }
}

#[test]
fn test_local_client_disconnect() {
    init_log();
    let mut server = setup_server();
    let client_id: SocketAddr = "127.0.0.0:0".parse().unwrap();
    let mut local_client = server.create_local_client(client_id);

    let connect_event = server.get_event().unwrap();
    if let ServerEvent::ClientConnected(id) = connect_event {
        assert_eq!(id, local_client.id());
    } else {
        unreachable!()
    }
    assert!(local_client.is_connected());
    local_client.disconnect();
    assert!(!local_client.is_connected());

    server.update();

    let disconnect_event = server.get_event().unwrap();
    if let ServerEvent::ClientDisconnected(id) = disconnect_event {
        assert_eq!(id, local_client.id());
    } else {
        unreachable!()
    }
}

#[test]
fn test_connection_permission_none() {
    init_log();
    let mut server = setup_server_with_config(Default::default(), ConnectionPermission::None);
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
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
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::Denied))
    ));

    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    server.allow_client(&remote_connection.id());
    connect_to_server(&mut server, &mut remote_connection).unwrap();
}

#[test]
fn test_connection_permission_denied() {
    init_log();
    let mut server = setup_server_with_config(Default::default(), ConnectionPermission::All);
    let mut remote_connection = request_remote_connection(server.addr().unwrap());
    server.deny_client(&remote_connection.id());
    let error = connect_to_server(&mut server, &mut remote_connection);
    assert!(matches!(
        error,
        Err(RenetError::ConnectionError(DisconnectionReason::Denied))
    ));
}

fn create_remote_connection(server_addr: SocketAddr) -> UdpClient {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config: ConnectionConfig = ConnectionConfig {
        timeout_duration: Duration::from_millis(100),
        heartbeat_time: Duration::from_millis(10),
        ..Default::default()
    };
    let request_connection = UdpClient::new(
        socket,
        server_addr,
        connection_config,
        reliable_channels_config(),
    )
    .unwrap();

    request_connection
}

struct ClientStatus {
    connection: UdpClient,
    received_messages: u64,
}

impl ClientStatus {
    pub fn new(connection: UdpClient) -> Self {
        Self {
            connection,
            received_messages: 0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TestUsage {
    value: Vec<u8>,
}

impl Default for TestUsage {
    fn default() -> Self {
        Self {
            value: vec![255; 1150],
        }
    }
}

use std::collections::HashMap;

#[test]
fn test_usage() {
    // TODO: we can't distinguish the log between the clients
    init_log();
    let mut server = setup_server();

    let mut clients_status: HashMap<u64, ClientStatus> = HashMap::new();
    let mut connected_clients = 0;
    let mut sent_messages = 0;

    for i in 0..8 {
        let remote_connection = create_remote_connection(server.addr().unwrap());
        let status = ClientStatus::new(remote_connection);
        clients_status.insert(i, status);
    }

    loop {
        for status in clients_status.values_mut() {
            status.connection.update().unwrap();
            if status.connection.connection_error().is_some() {
                panic!("Error in client connection");
            }
            if status.connection.is_connected() {
                if status.connection.receive_reliable_message(0).is_some() {
                    status.received_messages += 1;
                    if status.received_messages > 64 {
                        panic!("Received more than 64 messages!");
                    }
                }
            }
            status.connection.send_packets().unwrap()
        }

        if connected_clients == 8 && sent_messages < 64 {
            let message = bincode::options().serialize(&TestUsage::default()).unwrap();
            server.send_reliable_message(SendTarget::All, 0, message);
            sent_messages += 1
        }

        server.update();
        while let Some(event) = server.get_event() {
            if let ServerEvent::ClientConnected(_) = event {
                connected_clients += 1;
            }
        }
        server.send_packets().unwrap();

        clients_status.retain(|_, v| v.received_messages != 64);
        if clients_status.is_empty() {
            return;
        }
    }
}
