use crate::error::RenetError;
use crate::{connection::ClientId, endpoint::NetworkInfo};

mod client_connected;
mod client_host;
mod request_connection;

pub use client_connected::ClientConnected;
pub use client_host::HostClient;
pub(crate) use client_host::HostServer;
pub use request_connection::RequestConnection;

pub trait Client<C> {
    fn id(&self) -> ClientId;

    fn send_message(&mut self, channel_id: C, message: Box<[u8]>);

    fn receive_all_messages_from_channel(&mut self, channel_id: C) -> Vec<Box<[u8]>>;

    fn network_info(&mut self) -> &NetworkInfo;

    fn send_packets(&mut self) -> Result<(), RenetError>;

    fn process_events(&mut self) -> Result<(), RenetError>;
}
