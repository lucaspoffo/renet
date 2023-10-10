use std::sync::mpsc::{Receiver, Sender};

#[cfg(feature = "bevy")]
use bevy_utils::synccell::SyncCell;

mod client;
mod server;

pub use client::ChannelClientTransport;
pub use server::ChannelServerTransport;

#[cfg(feature = "bevy")]
struct Connection {
    sender: SyncCell<Sender<Vec<u8>>>,
    receiver: SyncCell<Receiver<Vec<u8>>>,
}

#[cfg(not(feature = "bevy"))]
struct Connection {
    sender: Sender<Vec<u8>>,
    receiver: Receiver<Vec<u8>>,
}

impl Connection {
    fn new(sender: Sender<Vec<u8>>, receiver: Receiver<Vec<u8>>) -> Self {
        #[cfg(feature = "bevy")]
        {
            Self {
                sender: SyncCell::new(sender),
                receiver: SyncCell::new(receiver),
            }
        }
        #[cfg(not(feature = "bevy"))]
        {
            Self { sender, receiver }
        }
    }

    #[cfg(feature = "bevy")]
    fn receiver(&mut self) -> &mut Receiver<Vec<u8>> {
        self.receiver.get()
    }

    #[cfg(feature = "bevy")]
    fn sender(&mut self) -> &mut Sender<Vec<u8>> {
        self.sender.get()
    }

    #[cfg(not(feature = "bevy"))]
    fn receiver(&mut self) -> &mut Receiver<Vec<u8>> {
        &mut self.receiver
    }

    #[cfg(not(feature = "bevy"))]
    fn sender(&mut self) -> &mut Sender<Vec<u8>> {
        &mut self.sender
    }
}
