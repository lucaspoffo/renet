pub mod channel;
pub mod client;
pub mod connection_control;
pub mod error;
mod packet;
pub mod protocol;
mod reassembly_fragment;
pub mod remote_connection;
mod sequence_buffer;
pub mod server;
mod timer;
pub mod transport;

pub trait ClientId: Clone + Copy + std::fmt::Debug + std::hash::Hash + Eq {}
impl<T> ClientId for T where T: Copy + std::fmt::Debug + std::hash::Hash + Eq {}
