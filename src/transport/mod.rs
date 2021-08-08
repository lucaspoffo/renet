use crate::connection_control::ConnectionControl;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::Payload;

use std::error::Error;

pub mod udp;

pub trait TransportClient {
    fn recv(&mut self) -> Result<Option<Payload>, RenetError>;
    fn update(&mut self);
    fn connection_error(&self) -> Option<DisconnectionReason>;
    fn send_to_server(&mut self, payload: &[u8]);
    fn disconnect(&mut self, reason: DisconnectionReason);
    fn is_connected(&self) -> bool;
}

pub trait TransportServer {
    type ClientId;
    type ConnectionId;

    fn recv(
        &mut self,
        connection_control: &ConnectionControl<Self::ClientId>,
    ) -> Result<Option<(Self::ClientId, Payload)>, Box<dyn Error + Send + Sync + 'static>>;

    fn send(
        &mut self,
        client_id: Self::ClientId,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;

    fn confirm_connect(&mut self, client_id: Self::ClientId);
    fn disconnect(&mut self, client_id: &Self::ClientId, reason: DisconnectionReason);
    fn is_authenticated(&self, client_id: Self::ClientId) -> bool;
    fn connection_id(&self) -> Self::ConnectionId;
    fn update(&mut self) -> Vec<Self::ClientId>;
}
