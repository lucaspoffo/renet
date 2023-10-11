use std::collections::HashMap;

use crossbeam::channel::{unbounded, TryRecvError};
use renet::{ClientId, RenetServer};

use crate::client::ChannelClientTransport;
use crate::Connection;

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
#[derive(Default)]
pub struct ChannelServerTransport {
    connections: HashMap<ClientId, Connection>,
    next_client_id: u64,
}

impl ChannelServerTransport {
    pub fn create_client(&mut self) -> ChannelClientTransport {
        let (send_to_client, receive_from_server) = unbounded::<Vec<u8>>();
        let (send_to_server, receive_from_client) = unbounded::<Vec<u8>>();

        let client_id = ClientId::from_raw(self.next_client_id);
        self.next_client_id += 1;

        let connection = Connection::new(send_to_client, receive_from_client);
        self.connections.insert(client_id, connection);

        ChannelClientTransport::new(client_id, receive_from_server, send_to_server)
    }

    pub fn update(&mut self, server: &mut RenetServer) {
        for (&client_id, connection) in self.connections.iter_mut() {
            server.add_connection(client_id);

            match connection.receiver.try_recv() {
                Ok(packet) => server.process_packet_from(&packet, client_id).unwrap(),
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    self.disconnect_client(client_id, server);
                    break;
                }
            }
        }
    }

    pub fn send_packets(&mut self, server: &mut RenetServer) {
        for client_id in server.clients_id() {
            let connection = self.connections.get_mut(&client_id).unwrap();

            let packets = server.get_packets_to_send(client_id).unwrap();

            for packet in packets {
                if connection.sender.send(packet).is_err() {
                    self.disconnect_client(client_id, server);
                    break;
                }
            }
        }
    }

    pub fn disconnect_client(&mut self, client_id: ClientId, server: &mut RenetServer) {
        self.connections.remove(&client_id);
        server.disconnect(client_id);
    }

    pub fn disconnect_all(&mut self, server: &mut RenetServer) {
        self.connections = HashMap::new();
        server.disconnect_all();
    }
}
