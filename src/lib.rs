pub mod channel;
pub mod client;
pub mod error;
mod packet;
pub mod protocol;
mod reassembly_fragment;
pub mod remote_connection;
mod sequence_buffer;
pub mod server;
mod timer;

pub(crate) use timer::Timer;

use std::{fmt::Debug, hash::Hash};

pub trait ClientId: Copy + Debug + Hash + Eq {}
impl<T> ClientId for T where T: Copy + Debug + Hash + Eq {}
