use crossbeam::channel::{Receiver, Sender, TryRecvError};
use renet::{ClientId, DisconnectReason, RenetClient};

use crate::Connection;

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct ChannelClientTransport {
    client_id: ClientId,
    connection: Option<Connection>,
}

impl ChannelClientTransport {
    pub(crate) fn new(client_id: ClientId, receiver: Receiver<Vec<u8>>, sender: Sender<Vec<u8>>) -> Self {
        Self {
            client_id,
            connection: Some(Connection::new(sender, receiver)),
        }
    }

    pub fn update(&mut self, client: &mut RenetClient) {
        let Some(ref mut connection) = self.connection else { return };
        client.set_connected();

        loop {
            match connection.receiver.try_recv() {
                Ok(packet) => {
                    client.process_packet(&packet);
                    continue;
                }
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    client.disconnect_due_to_transport();
                    self.disconnect(client)
                }
            }
            break;
        }
    }

    pub fn send_packets(&mut self, client: &mut RenetClient) {
        let packets = client.get_packets_to_send();

        for packet in packets {
            let Some(ref mut connection) = self.connection else { continue };

            if connection.sender.send(packet).is_err() {
                client.disconnect_due_to_transport();
                self.disconnect(client);
                break;
            }
        }
    }

    pub fn disconnect(&mut self, client: &mut RenetClient) {
        client.disconnect();
        self.connection.take();
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }
}
