mod block;
mod reliable;
mod unreliable;

use std::error::Error;

pub use block::{BlockChannel, BlockChannelConfig};
pub use reliable::{ReliableOrderedChannel, ReliableOrderedChannelConfig};
pub use unreliable::{UnreliableUnorderedChannel, UnreliableUnorderedChannelConfig};

use crate::packet::Payload;

pub trait ChannelConfig {
    fn new_channel(&self) -> Box<dyn Channel>;
}

pub trait Channel {
    fn get_messages_to_send(&mut self, available_bytes: u32, sequence: u16)
        -> Option<Vec<Payload>>;
    fn process_messages(&mut self, messages: Vec<Payload>);
    fn process_ack(&mut self, ack: u16);
    fn send_message(
        &mut self,
        message_payload: Payload,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>>;
    fn receive_message(&mut self) -> Option<Payload>;
}
