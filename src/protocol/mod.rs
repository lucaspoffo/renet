use crate::{error::DisconnectionReason, packet::Payload, ClientId};
use std::error::Error;

pub mod unsecure;

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync + 'static>>;

pub trait AuthenticationProtocol {
    type ClientId: ClientId;

    fn create_payload(&mut self) -> Result<Option<Vec<u8>>>;
    fn process(&mut self, payload: &[u8]) -> Option<Payload>;
    fn wrap(&mut self, payload: &[u8]) -> Result<Payload>;
    fn is_authenticated(&self) -> bool;
    fn id(&self) -> Self::ClientId;
    fn disconnected(&self) -> Option<DisconnectionReason>;
}

pub trait ServerAuthenticationProtocol: AuthenticationProtocol + Sized {
    fn get_disconnect_packet(reason: DisconnectionReason) -> Payload;
    fn from_payload(payload: &[u8]) -> Result<Self>;
}
