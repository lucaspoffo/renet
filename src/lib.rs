use self::sequence_buffer::SequenceBuffer;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Cursor, Write};

mod sequence_buffer;

#[derive(Debug)]
pub enum RenetError {
    MaximumFragmentsExceeded,
    CouldNotFindFragment,
    InvalidNumberFragment,
    FragmentAlreadyProcessed,
    InvalidHeaderType,
    IOError(io::Error),
}

impl From<io::Error> for RenetError {
    fn from(inner: io::Error) -> RenetError {
        RenetError::IOError(inner)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PacketType {
    Packet = 0,
    Fragment = 1,
}

pub struct Packet<'a> {
    packet_type: PacketType,
    payload: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PacketHeader {
    // protocol_id: u16,
    // crc32: u32, // append protocol_id when calculating crc32
    sequence: u16,
}

impl HeaderWriter for PacketHeader {
    type Output = Result<(), RenetError>;

    fn write(&self, buffer: &mut Vec<u8>) -> Self::Output {
        buffer.write_u8(PacketType::Packet as u8)?;
        buffer.write_u16::<BigEndian>(self.sequence)?;
        Ok(())
    }
}

impl HeaderReader for PacketHeader {
    type Header = Result<PacketHeader, RenetError>;

    fn size() -> u8 {
       3 
    }

    fn read(rdr: &mut Cursor<&[u8]>) -> Self::Header {
        let packet_type = rdr.read_u8()?;
        if packet_type != PacketType::Packet as u8 {
            return Err(RenetError::InvalidHeaderType);
        }
        let sequence = rdr.read_u16::<BigEndian>()?;

        let header = PacketHeader {
            sequence,
        };

        Ok(header)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FragmentHeader {
    // crc32: u32,
    sequence: u16,
    fragment_id: u8,
    num_fragments: u8,
}

trait HeaderReader {
    type Header;

    fn read(rdr: &mut Cursor<&[u8]>) -> Self::Header;

    /// Header size in bytes
    fn size() -> u8;
}

trait HeaderWriter {
    type Output;

    fn write(&self, buffer: &mut Vec<u8>) -> Self::Output;
}

impl HeaderWriter for FragmentHeader {
    type Output = Result<(), RenetError>;

    fn write(&self, buffer: &mut Vec<u8>) -> Self::Output {
        buffer.write_u8(PacketType::Fragment as u8)?;
        buffer.write_u16::<BigEndian>(self.sequence)?;
        buffer.write_u8(self.fragment_id)?;
        buffer.write_u8(self.num_fragments)?;
        Ok(())
    }
}

impl HeaderReader for FragmentHeader {
    type Header = Result<FragmentHeader, RenetError>;

    fn size() -> u8 {
       5 
    }

    fn read(rdr: &mut Cursor<&[u8]>) -> Self::Header {
        let packet_type = rdr.read_u8()?;
        if packet_type != PacketType::Fragment as u8 {
            return Err(RenetError::InvalidHeaderType);
        }
        let sequence = rdr.read_u16::<BigEndian>()?;
        let fragment_id = rdr.read_u8()?;
        let num_fragments = rdr.read_u8()?;

        let header = FragmentHeader {
            sequence,
            fragment_id,
            num_fragments,
        };

        Ok(header)
    }
}

const FRAGMENT_MAX_COUNT: usize = 256;
const FRAGMENT_MAX_SIZE: usize = 1024;

#[derive(Clone)]
struct ReassemblyFragment {
    sequence: u16,
    num_fragments_received: usize,
    num_fragments_total: usize,
    buffer: Vec<u8>,
    fragments_received: [bool; FRAGMENT_MAX_COUNT],
}

impl Default for ReassemblyFragment {
    fn default() -> Self {
        Self {
            sequence: 0,
            num_fragments_received: 0,
            num_fragments_total: 0,
            buffer: Vec::with_capacity(0),
            fragments_received: [false; FRAGMENT_MAX_COUNT],
        }
    }
}

impl ReassemblyFragment {
    pub fn new(sequence: u16, num_fragments_total: usize) -> Self {
        Self {
            sequence,
            num_fragments_received: 0,
            num_fragments_total,
            buffer: Vec::with_capacity(num_fragments_total * FRAGMENT_MAX_SIZE),
            fragments_received: [false; FRAGMENT_MAX_COUNT],
        }
    }
}

struct Config {
    max_packet_size: usize,
    max_fragments: usize,
    fragment_above: usize,
    fragment_size: usize,
    fragment_reassembly_buffer_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_packet_size: 16 * 1024,
            max_fragments: 32,
            fragment_above: 1024,
            fragment_size: 1024,
            fragment_reassembly_buffer_size: 256,
        }
    }
}

pub fn build_fragments<'a>(payload: &'a [u8]) -> Result<Vec<&'a [u8]>, RenetError> {
    let config = Config::default();
    let packet_bytes = payload.len();
    let exact_division = ((packet_bytes % config.fragment_size) != 0) as usize;
    let num_fragments = packet_bytes / config.fragment_size + exact_division;

    if num_fragments > config.max_fragments {
        return Err(RenetError::MaximumFragmentsExceeded);
    }

    let mut fragments = Vec::with_capacity(num_fragments);
    for id in 0..num_fragments {
        let start = config.fragment_size * id;
        let mut end = config.fragment_size * (id + 1);
        if packet_bytes < end {
            end = packet_bytes;
        }
        let data = &payload[start..end];
        fragments.push(data);
    }

    Ok(fragments)
}

impl SequenceBuffer<ReassemblyFragment> {
    pub fn handle_fragment(
        &mut self,
        header: FragmentHeader,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, RenetError> {
        if !self.exists(header.sequence) {
            let reassembly_fragment =
                ReassemblyFragment::new(header.sequence, header.num_fragments as usize);
            self.insert(header.sequence, reassembly_fragment);
        }

        let reassembly_fragment = match self.get_mut(header.sequence) {
            Some(x) => x,
            None => return Err(RenetError::CouldNotFindFragment),
        };

        if reassembly_fragment.num_fragments_total != header.num_fragments as usize {
            return Err(RenetError::InvalidNumberFragment);
        }

        if header.fragment_id as usize >= reassembly_fragment.num_fragments_total {
            return Err(RenetError::MaximumFragmentsExceeded);
        }

        if reassembly_fragment.fragments_received[header.fragment_id as usize] {
            return Err(RenetError::FragmentAlreadyProcessed);
        }

        reassembly_fragment.num_fragments_received += 1;
        reassembly_fragment.fragments_received[header.fragment_id as usize] = true;
        reassembly_fragment.buffer.write_all(&*payload)?;

        if reassembly_fragment.num_fragments_received == reassembly_fragment.num_fragments_total {
            drop(reassembly_fragment);
            if let Some(reassembly_fragment) = self.remove(header.sequence) {
                return Ok(Some(reassembly_fragment.buffer.clone()));
            } else {
                return Err(RenetError::CouldNotFindFragment);
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn fragment_header_read_write() {
        let fragment_header = FragmentHeader {
            sequence: 42,
            fragment_id: 3,
            num_fragments: 5
        };
        
        let mut buffer = vec![];
        
        fragment_header.write(&mut buffer).unwrap();
        let mut cursor = Cursor::new(buffer.as_slice());
        let parsed_fragment_header = FragmentHeader::read(&mut cursor).unwrap(); 
        assert_eq!(fragment_header, parsed_fragment_header);
    }
    
    #[test]
    fn packet_header_read_write() {
        let header = PacketHeader {
            sequence: 42,
        };
        
        let mut buffer = vec![];
        
        header.write(&mut buffer).unwrap();
        let mut cursor = Cursor::new(buffer.as_slice());
        let parsed_header = PacketHeader::read(&mut cursor).unwrap(); 
        assert_eq!(header, parsed_header);
    }


    #[test]
    fn fragment() {
        let payload = [7u8; 3500];
        let fragments = build_fragments(&payload).unwrap();
        let mut fragments_reassembly: SequenceBuffer<ReassemblyFragment> =
            SequenceBuffer::with_capacity(256);
        let mut headers = vec![];
        for i in 0..fragments.len() {
            let header = FragmentHeader {
                sequence: 0,
                fragment_id: i as u8,
                num_fragments: fragments.len() as u8,
            };
            headers.push(header);
        }
        assert_eq!(4, fragments.len());
        assert!(fragments_reassembly
            .handle_fragment(headers[0].clone(), fragments[0])
            .unwrap()
            .is_none());
        assert!(fragments_reassembly
            .handle_fragment(headers[1].clone(), fragments[1])
            .unwrap()
            .is_none());
        assert!(fragments_reassembly
            .handle_fragment(headers[2].clone(), fragments[2])
            .unwrap()
            .is_none());
        let reassembly_payload = fragments_reassembly
            .handle_fragment(headers[3].clone(), fragments[3])
            .unwrap()
            .unwrap();
        assert_eq!(reassembly_payload.len(), payload.len());

        assert!(reassembly_payload
            .iter()
            .zip(payload.iter())
            .all(|(a, b)| a == b));
    }
}
