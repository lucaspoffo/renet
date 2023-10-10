use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use renet::RenetClient;

use crate::Connection;

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct ChannelClientTransport {
    connection: Option<Connection>,
}

impl ChannelClientTransport {
    pub(crate) fn new(receiver: Receiver<Vec<u8>>, sender: Sender<Vec<u8>>) -> Self {
        Self {
            connection: Some(Connection::new(sender, receiver)),
        }
    }

    pub fn update(&mut self, client: &mut RenetClient) {
        let Some(ref mut connection) = self.connection else { return };

        loop {
            match connection.receiver().try_recv() {
                Ok(packet) => {
                    client.process_packet(&packet);
                    continue;
                }
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => self.disconnect(client),
            }
            break;
        }
    }

    pub fn send_packets(&mut self, client: &mut RenetClient) {
        let packets = client.get_packets_to_send();

        for packet in packets {
            let Some(ref mut connection) = self.connection else { continue };

            if connection.sender().send(packet).is_err() {
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
}
