use crate::ClientId;
use std::error::Error;

pub mod unsecure;

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync + 'static>>;

pub trait SecurityService {
    fn ss_wrap(&mut self, data: &[u8]) -> Result<Vec<u8>>;
    fn ss_unwrap(&mut self, data: &[u8]) -> Result<Vec<u8>>;
}

pub trait AuthenticationProtocol {
    type Service: SecurityService;
    type ClientId: ClientId;

    fn create_payload(&mut self) -> Result<Option<Vec<u8>>>;
    fn read_payload(&mut self, payload: &[u8]) -> Result<()>;
    fn is_authenticated(&self) -> bool;
    fn build_security_interface(&self) -> Self::Service;
    fn id(&self) -> Self::ClientId;
}

pub trait ServerAuthenticationProtocol: AuthenticationProtocol + Sized {
    fn from_payload(payload: &[u8]) -> Result<Self>;
}
