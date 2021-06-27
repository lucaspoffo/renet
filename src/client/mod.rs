use crate::error::RenetError;
use crate::remote_connection::{ClientId, NetworkInfo};

mod remote_client;
mod local_client;
mod request_connection;

pub use remote_client::RemoteClientConnected;
pub(crate) use local_client::LocalClient;
pub use local_client::LocalClientConnected;
pub use request_connection::RequestConnection;

pub trait Client {
    fn id(&self) -> ClientId;

    fn send_message(&mut self, channel_id: u8, message: Box<[u8]>);

    fn receive_message(&mut self, channel_id: u8) -> Option<Box<[u8]>>;

    fn network_info(&mut self) -> &NetworkInfo;

    fn send_packets(&mut self) -> Result<(), RenetError>;

    fn process_events(&mut self) -> Result<(), RenetError>;
}
