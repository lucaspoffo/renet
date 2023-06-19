pub mod transport {
    use std::time::Duration;

    use renet::{RenetClient, RenetServer};

    const MAX_MESSAGE_BATCH_SIZE: usize = 255;

    pub trait Transport<T = (RenetServer, RenetClient)> {
        // TODO duration may not be needed
        fn update(&mut self, duration: Duration, handler: &mut T);
        fn send_packets(&mut self, handler: &mut T);
    }
    pub mod client;
    pub mod server;
}
