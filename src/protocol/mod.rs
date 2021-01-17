use crate::connection::ClientId;
use crate::error::Result;

pub mod unsecure;

pub trait SecurityService {
    fn ss_wrap(&mut self, data: &[u8]) -> Result<Box<[u8]>>;
    fn ss_unwrap(&mut self, data: &[u8]) -> Result<Box<[u8]>>;
}

pub trait AuthenticationProtocol {
    fn create_payload(&mut self) -> Result<Option<Box<[u8]>>>;
    fn read_payload(&mut self, payload: &[u8]) -> Result<()>;
    fn is_authenticated(&self) -> bool;
    fn build_security_interface(&self) -> Box<dyn SecurityService>;
    fn id(&self) -> ClientId;
}

// TODO: review name
pub trait ServerAuthenticationProtocol {
    fn from_payload(payload: &[u8]) -> Result<Box<dyn AuthenticationProtocol>>;
    // TODO: add deny connection fn
}
