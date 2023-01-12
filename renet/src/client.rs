use crate::{
    error::{DisconnectionReason, RenetError},
    network_info::{ClientPacketInfo, NetworkInfo, PacketInfo},
    RenetConnectionConfig,
};

use rechannel::{remote_connection::RemoteConnection, Bytes};
use std::time::Duration;

/// A client that establishes an authenticated connection with a server.
/// Can send/receive encrypted messages from/to the server.
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct RenetClient {
    reliable_connection: RemoteConnection,
    client_packet_info: ClientPacketInfo,
}

impl RenetClient {
    pub fn new(config: RenetConnectionConfig) -> Self {
        let reliable_connection = RemoteConnection::new(config.to_connection_config());
        let client_packet_info = ClientPacketInfo::new(config.bandwidth_smoothing_factor);

        Self {
            reliable_connection,
            client_packet_info,
        }
    }

    #[doc(hidden)]
    pub fn __test() -> Self {
        Self::new(Default::default())
    }

    /// If the client is disconnected, returns the reason.
    pub fn disconnected(&self) -> Option<DisconnectionReason> {
        if let Some(reason) = self.reliable_connection.disconnected() {
            return Some(reason.into());
        }

        None
    }

    /// Receive a message from the server over a channel.
    pub fn receive_message<I: Into<u8>>(&mut self, channel_id: I) -> Option<Vec<u8>> {
        self.reliable_connection.receive_message(channel_id)
    }

    /// Send a message to the server over a channel.
    pub fn send_message<I: Into<u8>, B: Into<Bytes>>(&mut self, channel_id: I, message: B) {
        self.reliable_connection.send_message(channel_id, message);
    }

    /// Verifies if a message can be sent to the server over a channel.
    pub fn can_send_message<I: Into<u8>>(&self, channel_id: I) -> bool {
        self.reliable_connection.can_send_message(channel_id)
    }

    pub fn network_info(&self) -> NetworkInfo {
        NetworkInfo {
            sent_kbps: self.client_packet_info.sent_kbps,
            received_kbps: self.client_packet_info.received_kbps,
            rtt: self.reliable_connection.rtt(),
            packet_loss: self.reliable_connection.packet_loss(),
        }
    }

    pub fn advance_time(&mut self, duration: Duration) {
        self.reliable_connection.advance_time(duration);
    }

    pub fn process_packet() {}

    /// Advances the client by duration, and receive packets from the network.
    pub fn update(&mut self) -> Result<(), RenetError> {
        self.reliable_connection.update()?;
        self.client_packet_info.update_metrics();

        Ok(())
    }
}
