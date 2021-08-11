use crate::error::{DisconnectionReason, MessageError, RenetError};
use crate::packet::Payload;
use crate::remote_connection::NetworkInfo;

mod local_client;
mod remote_client;

pub(crate) use local_client::LocalClient;
pub use local_client::LocalClientConnected;
pub use remote_client::RemoteClient;

pub trait Client<C> {
    fn id(&self) -> C;

    fn is_connected(&self) -> bool;

    fn connection_error(&self) -> Option<DisconnectionReason>;

    fn disconnect(&mut self);

    fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), MessageError>;

    fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, MessageError>;

    fn network_info(&self) -> &NetworkInfo;

    fn send_packets(&mut self) -> Result<(), RenetError>;

    fn update(&mut self) -> Result<(), RenetError>;
}
