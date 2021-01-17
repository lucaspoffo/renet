use crate::connection::ClientId;
use crate::error::Result;
use crate::protocol::AuthenticationProtocol;
use crate::Timer;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub struct HandleConnection {
    pub(crate) addr: SocketAddr,
    pub(crate) client_id: ClientId,
    pub(crate) protocol: Box<dyn AuthenticationProtocol>,
    timeout_timer: Timer,
}

impl HandleConnection {
    pub fn new(
        client_id: ClientId,
        addr: SocketAddr,
        protocol: Box<dyn AuthenticationProtocol>,
        timeout_duration: Duration,
    ) -> Self {
        let timeout_timer = Timer::new(timeout_duration);
        Self {
            client_id,
            addr,
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
