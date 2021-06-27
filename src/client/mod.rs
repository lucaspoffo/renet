use crate::error::RenetError;
use crate::remote_connection::{ClientId, NetworkInfo};

mod client_connected;
mod client_host;
mod request_connection;

pub use client_connected::RemoteClientConnected;
pub(crate) use client_host::LocalClient;
pub use client_host::LocalClientConnected;
pub use request_connection::RequestConnection;

pub trait Client {
    fn id(&self) -> ClientId;

    fn send_message(&mut self, channel_id: u8, message: Box<[u8]>);

    fn receive_message(&mut self, channel_id: u8) -> Option<Box<[u8]>>;

    fn network_info(&mut self) -> &NetworkInfo;

    fn send_packets(&mut self) -> Result<(), RenetError>;

    fn process_events(&mut self) -> Result<(), RenetError>;
}
