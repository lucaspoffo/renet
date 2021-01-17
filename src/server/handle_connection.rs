use crate::connection::ClientId;
use crate::error::Result;
use crate::protocol::AuthenticationProtocol;
use std::net::SocketAddr;

pub struct HandleConnection {
    pub(crate) addr: SocketAddr,
    pub(crate) client_id: ClientId,
    pub(crate) protocol: Box<dyn AuthenticationProtocol>,
}

impl HandleConnection {
    pub fn new(
        client_id: ClientId,
        addr: SocketAddr,
        protocol: Box<dyn AuthenticationProtocol>,
    ) -> Self {
        Self {
            client_id,
            addr,
            protocol,
        }
    }

    pub fn process_payload(&mut self, payload: &[u8]) -> Result<()> {
        self.protocol.read_payload(payload)
    }
}
