use crate::error::Result;
use crate::protocol::AuthenticationProtocol;
use crate::remote_connection::ClientId;
use crate::Timer;

use std::net::SocketAddr;
use std::time::Duration;

pub struct HandleConnection<P> {
    pub(crate) addr: SocketAddr,
    pub(crate) client_id: ClientId,
    pub(crate) protocol: P,
    timeout_timer: Timer,
}

impl<P: AuthenticationProtocol> HandleConnection<P> {
    pub fn new(
        client_id: ClientId,
        addr: SocketAddr,
        protocol: P,
        timeout_duration: Duration,
    ) -> Self {
        let timeout_timer = Timer::new(timeout_duration);
        Self {
            addr,
            client_id,
            protocol,
            timeout_timer,
        }
    }

    pub fn process_payload(&mut self, payload: &[u8]) -> Result<()> {
        self.timeout_timer.reset();
        self.protocol.read_payload(payload)
    }

    pub fn has_timed_out(&mut self) -> bool {
        self.timeout_timer.is_finished()
    }
}
