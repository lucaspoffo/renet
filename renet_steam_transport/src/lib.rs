const MTU_UNRELIABLE: usize = 1200; // 1200 bytes for unreliable packets before they got split by Steam
const MTU_RELIABLE: usize = 512 * 1024; // 512 KB for reliable packets, they may accept more by recieve but this is the max size of a single packet the they can send

pub mod transport {
    pub mod client;
    pub mod server;
}
