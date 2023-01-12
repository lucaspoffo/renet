use crate::{remote_connection::{RemoteConnection, ConnectionConfig}, transport::ClientTransport};

pub struct RechannelClient {
    connection: RemoteConnection
}


impl RechannelClient {
    pub fn new(config: ConnectionConfig) -> Self {
        let connection = RemoteConnection::new(config);

        Self {
            connection
        }
    }

    pub fn send_packets(&mut self, transport: &mut dyn ClientTransport) {
        let packets = match self.connection.get_packets_to_send() {
            Ok(p) => p,
            Err(e) => {
                log::error!("Failed to get packets to send: {}", e);
                return;
            },
        };

        for packet in packets.iter() {
           transport.send(packet);
        }
    }
}