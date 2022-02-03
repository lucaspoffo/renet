pub mod channel;
pub mod error;
mod packet;
mod reassembly_fragment;
pub mod remote_connection;
mod sequence_buffer;
pub mod server;
mod timer;

pub use packet::disconnect_packet;

pub use reassembly_fragment::FragmentConfig;

use std::{fmt::Debug, hash::Hash};

pub trait ClientId: Copy + Debug + Hash + Eq {}
impl<T> ClientId for T where T: Copy + Debug + Hash + Eq {}
