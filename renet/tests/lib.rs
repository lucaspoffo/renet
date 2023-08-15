use bytes::Bytes;
use renet::{ConnectionConfig, DefaultChannel, NetworkId, RenetClient, RenetServer};

pub fn init_log() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[test]
fn test_remote_connection_reliable_channel() {
    init_log();
    let mut server = RenetServer::new(ConnectionConfig::default());
    let mut client = RenetClient::new(ConnectionConfig::default());

    let client_id = NetworkId::from_raw(0);
    server.add_connection(client_id);

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
}
