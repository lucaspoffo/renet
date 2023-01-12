pub mod channel;
mod circular_buffer;
pub mod error;
mod network_info;
mod packet;
mod reassembly_fragment;
pub mod remote_connection;
mod sequence_buffer;
pub mod server;
mod timer;
pub mod transport;
pub mod client;

pub use bytes::Bytes;
pub use network_info::NetworkInfo;
pub use packet::serialize_disconnect_packet;
pub use reassembly_fragment::FragmentConfig;

use std::{fmt::Debug, hash::Hash};

pub trait ClientId: Copy + Debug + Hash + Eq {}
impl<T> ClientId for T where T: Copy + Debug + Hash + Eq {}

// Reused in the renet_visualizer crate
#[doc(hidden)]
pub use circular_buffer::CircularBuffer;
