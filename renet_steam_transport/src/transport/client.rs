use std::time::Duration;

use renet::RenetClient;
use steamworks::{
    networking_sockets::{InvalidHandle, NetConnection},
    networking_types::{NetworkingIdentity, SendFlags},
    ClientManager, SteamId,
};

use super::{Transport, MAX_MESSAGE_BATCH_SIZE};
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]

pub struct Client {
    connection: NetConnection<ClientManager>,
}

impl Client {
    /// Create a new client connection to the server
    ///
    /// If the connection is not possible, it will return [`InvalidHandle`](steamworks::networking_sockets)
    pub fn new(steam_client: &steamworks::Client<ClientManager>, steam_id: &SteamId) -> Result<Self, InvalidHandle> {
        let options = Vec::new();
        match steam_client
            .networking_sockets()
            .connect_p2p(NetworkingIdentity::new_steam_id(*steam_id), 0, options)
        {
            Ok(connection) => Ok(Self { connection }),
            Err(h) => Err(h),
        }
    }

    pub fn client_id(&self, steam_client: &steamworks::Client<ClientManager>) -> u64 {
        steam_client.user().steam_id().raw()
    }

    pub fn disconnect(self, send_last_packets: bool) {
        self.connection.close(
            steamworks::networking_types::NetConnectionEnd::AppGeneric,
            Some("Disconnecting from server"),
            send_last_packets,
        );
    }
}

impl Transport<RenetClient> for Client {
    fn update(&mut self, _duration: Duration, client: &mut RenetClient) {
        let messages = self.connection.receive_messages(MAX_MESSAGE_BATCH_SIZE);
        messages.iter().for_each(|message| {
            client.process_packet(message.data());
        });
    }

    fn send_packets(&mut self, client: &mut RenetClient) {
        let packets = client.get_packets_to_send();
        for packet in packets {
            if let Err(e) = self.connection.send_message(&packet, SendFlags::UNRELIABLE) {
                log::error!("Error while sending packet: {}", e);
            }
        }
        match self.connection.flush_messages() {
            Err(e) => log::error!("Error while flushing messages: {}", e),
            _ => (),
        }
    }
}
