use crate::error::{ConnectionError, RenetError};
use crate::packet::Payload;
use crate::remote_connection::{ClientId, NetworkInfo};

mod local_client;
mod remote_client;

pub(crate) use local_client::LocalClient;
pub use local_client::LocalClientConnected;
pub use remote_client::RemoteClient;

pub trait Client {
    fn id(&self) -> ClientId;

    fn is_connected(&self) -> bool;

    fn connection_error(&self) -> Option<ConnectionError>;

    fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), RenetError>;

    fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, RenetError>;

    fn network_info(&mut self) -> &NetworkInfo;

    fn send_packets(&mut self) -> Result<(), RenetError>;

    fn update(&mut self) -> Result<(), RenetError>;
}
