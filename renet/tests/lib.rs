use bytes::Bytes;
use renet::{ClientId, ConnectionConfig, DefaultChannel, DisconnectReason, RenetClient, RenetServer, ServerEvent};

pub fn init_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[test]
fn test_remote_connection_reliable_channel() {
    init_log();
    let mut server = RenetServer::new(ConnectionConfig::default());
    let mut client = RenetClient::new(ConnectionConfig::default());

    let client_id: ClientId = 0;
    server.add_connection(client_id);
    assert_eq!(server.connected_clients(), 1);
    assert!(server.has_connections());
    assert_eq!(ServerEvent::ClientConnected { client_id }, server.get_event().unwrap());

    for _ in 0..200 {
        server.send_message(client_id, DefaultChannel::ReliableOrdered, Bytes::from("test"));
    }

    let mut count = 0;
    let packets = server.get_packets_to_send(client_id).unwrap();
    for packet in packets.into_iter() {
        assert!(packet.len() < 1300);
        client.process_packet(&packet);
    }

    assert_eq!(client.disconnect_reason(), None);

    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        assert_eq!(message, "test");
        count += 1;
    }

    assert_eq!(count, 200);

    // Sliced messages
    let message = Bytes::from("test".repeat(1000));
    let mut count = 0;
    for _ in 0..10 {
        server.send_message(client_id, DefaultChannel::ReliableOrdered, message.clone());
    }

    let packets = server.get_packets_to_send(client_id).unwrap();
    for packet in packets.into_iter() {
        assert!(packet.len() < 1300);
        client.process_packet(&packet);
    }

    while let Some(received_message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        assert_eq!(received_message, message);
        count += 1;
    }

    assert_eq!(count, 10);

    server.remove_connection(client_id);
    assert_eq!(server.connected_clients(), 0);
    assert!(!server.has_connections());
    assert_eq!(
        ServerEvent::ClientDisconnected {
            client_id,
            reason: DisconnectReason::Transport
        },
        server.get_event().unwrap()
    );
}

#[test]
fn test_local_client() {
    init_log();
    let mut server = RenetServer::new(ConnectionConfig::default());

    let client_id: ClientId = 0;
    let mut client = server.new_local_client(client_id);

    let connect_event = server.get_event().unwrap();
    assert!(connect_event == ServerEvent::ClientConnected { client_id });

    server.send_message(client_id, DefaultChannel::ReliableOrdered, Bytes::from("test server"));
    client.send_message(DefaultChannel::ReliableOrdered, Bytes::from("test client"));

    server.process_local_client(client_id, &mut client).unwrap();

    let server_message = server.receive_message(client_id, DefaultChannel::ReliableOrdered).unwrap();
    assert_eq!(server_message, "test client");

    let client_message = client.receive_message(DefaultChannel::ReliableOrdered).unwrap();
    assert_eq!(client_message, "test server");

    server.disconnect_local_client(client_id, &mut client);
    assert!(client.is_disconnected());
    let disconnect_event = server.get_event().unwrap();
    assert!(
        disconnect_event
            == ServerEvent::ClientDisconnected {
                client_id,
                reason: DisconnectReason::DisconnectedByClient
            }
    );
}
