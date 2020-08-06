#[derive(Debug)]
pub enum ReliableError {
    SequenceBufferFull,
}

enum PacketType {
    Packet = 0,
    Fragment = 1,
}

pub struct Packet<'a> {
    packet_type: PacketType,
    payload: &'a [u8],
}

struct StandardHeader {
    // protocol_id: u16,
    crc32: u32, // append protocol_id when calculating crc32
    packet_type: PacketType,
}

struct FragmentHeader {
    crc32: u32,
    sequence: u16,
    packet_type: PacketType,
    fragment_id: u8,
    num_fragments: u8,
}

const MAX_FRAGMENT_SIZE: usize = 256;

struct ReassemblyFragment {
    sequence: u16,
    num_fragments_received: usize,
    num_fragments_total: usize,
    buffer: Vec<u8>,
    fragments_received: [bool; MAX_FRAGMENT_SIZE],
    header_size: usize,
}

