use crossbeam::channel::{Receiver, Sender};

pub use client::ChannelClientTransport;
pub use server::ChannelServerTransport;

mod client;
mod server;

struct Connection {
    sender: Sender<Vec<u8>>,
    receiver: Receiver<Vec<u8>>,
}

impl Connection {
    fn new(sender: Sender<Vec<u8>>, receiver: Receiver<Vec<u8>>) -> Self {
        Self { sender, receiver }
    }
}
