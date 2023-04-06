pub mod channels;
pub mod error;
mod packet;
pub mod remote_connection;
pub mod server;

pub use bytes::Bytes;

use std::{fmt::Debug, hash::Hash};

pub trait ClientId: Copy + Debug + Hash + Eq {}
impl<T> ClientId for T where T: Copy + Debug + Hash + Eq {}
