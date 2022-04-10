use rechannel::{
    disconnect_packet,
    error::DisconnectionReason,
    remote_connection::{ConnectionConfig, RemoteConnection},
    server::RechannelServer,
};

use bincode::{self, Options};
use serde::{Deserialize, Serialize};

use std::time::Duration;

pub fn init_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TestMessage {
    value: u64,
}

#[test]
fn test_remote_connection_reliable_channel() {
    init_log();
    let mut server = RechannelServer::new(ConnectionConfig::default());
    let mut client = RemoteConnection::new(ConnectionConfig::default());
    let client_id = 0u64;
    server.add_connection(&client_id);

    let number_messages = 100;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::options().serialize(&message).unwrap();
        server.send_message(&client_id, 0, message).unwrap();
    }

    loop {
        let packets = server.get_packets_to_send(&client_id).unwrap();
        for packet in packets.into_iter() {
            client.process_packet(&packet).unwrap();
        }

        while let Some(message) = client.receive_message(0) {
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
fn test_server_reliable_channel() {
    init_log();
    let mut server = RechannelServer::new(ConnectionConfig::default());
    let mut client = RemoteConnection::new(ConnectionConfig::default());
    let client_id = 0u64;
    server.add_connection(&client_id);

    let number_messages = 100;
    let mut current_message_number = 0;

    for i in 0..number_messages {
        let message = TestMessage { value: i };
        let message = bincode::options().serialize(&message).unwrap();
        client.send_message(0, message).unwrap();
    }

    loop {
        let packets = client.get_packets_to_send().unwrap();
        for packet in packets.into_iter() {
            server.process_packet_from(&packet, &client_id).unwrap();
        }

        while let Some(message) = server.receive_message(&client_id, 0) {
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
fn test_server_disconnect_client() {
    init_log();
    let mut server = RechannelServer::new(ConnectionConfig::default());
    let mut client = RemoteConnection::new(ConnectionConfig::default());
    let client_id = 0u64;
    server.add_connection(&client_id);

    server.disconnect(&client_id);

    let (_, reason) = server.disconnected_client().unwrap();
    assert_eq!(reason, DisconnectionReason::DisconnectedByServer);

    let packet = disconnect_packet(reason).unwrap();
    client.process_packet(&packet).unwrap();

    let client_reason = client.disconnected().unwrap();
    assert_eq!(reason, client_reason);
}

#[test]
fn test_client_disconnect() {
    init_log();
    let mut server = RechannelServer::new(ConnectionConfig::default());
    let mut client = RemoteConnection::new(ConnectionConfig::default());
    let client_id = 0u64;
    server.add_connection(&client_id);

    client.disconnect();
    let reason = client.disconnected().unwrap();
    assert_eq!(reason, DisconnectionReason::DisconnectedByClient);

    let packet = disconnect_packet(reason).unwrap();
    server.process_packet_from(&packet, &client_id).unwrap();
    server.update_connections(Duration::ZERO);

    let (disconnect_id, server_reason) = server.disconnected_client().unwrap();

    assert_eq!(client_id, disconnect_id);
    assert_eq!(reason, server_reason);
}

struct ClientStatus {
    connection: RemoteConnection,
    received_messages: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestUsage {
    value: Vec<u8>,
}

impl Default for TestUsage {
    fn default() -> Self {
        Self { value: vec![255; 400] }
    }
}

use std::collections::HashMap;

#[test]
fn test_usage() {
    // TODO: we can't distinguish the log between the clients
    init_log();
    let mut server = RechannelServer::new(ConnectionConfig::default());

    let mut clients_status: HashMap<usize, ClientStatus> = HashMap::new();
    let mut sent_messages = 0;

    for i in 0..8 {
        let connection = RemoteConnection::new(ConnectionConfig::default());
        let status = ClientStatus {
            connection,
            received_messages: 0,
        };
        clients_status.insert(i, status);
        server.add_connection(&i);
    }

    let mut count: u64 = 0;
    loop {
        count += 1;
        for (connection_id, status) in clients_status.iter_mut() {
            status.connection.update().unwrap();
            if status.connection.receive_message(0).is_some() {
                status.received_messages += 1;
                if status.received_messages > 32 {
                    panic!("Received more than 32 messages!");
                }
            }
            if status.received_messages == 32 {
                status.connection.disconnect();
                let reason = status.connection.disconnected().unwrap();
                let packet = disconnect_packet(reason).unwrap();
                server.process_packet_from(&packet, connection_id).unwrap();
                continue;
            }

            let client_packets = status.connection.get_packets_to_send().unwrap();
            let server_packets = server.get_packets_to_send(connection_id).unwrap();

            // 66% packet loss emulation
            if count % 3 == 0 {
                for packet in client_packets.iter() {
                    server.process_packet_from(packet, connection_id).unwrap();
                }
                for packet in server_packets.iter() {
                    status.connection.process_packet(packet).unwrap();
                }
            }

            status.connection.advance_time(Duration::from_millis(100));
        }

        server.update_connections(Duration::from_millis(100));
        clients_status.retain(|_, c| c.connection.is_connected());

        if sent_messages < 32 {
            let message = bincode::options().serialize(&TestUsage::default()).unwrap();
            server.broadcast_message(0, message);
            sent_messages += 1
        }

        if !server.has_connections() {
            return;
        }
    }
}
