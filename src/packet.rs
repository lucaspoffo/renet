use super::error::{Result, RenetError};
use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

pub const FRAGMENT_MAX_COUNT: usize = 256;
pub const FRAGMENT_MAX_SIZE: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PacketType {
    Packet = 0,
    Fragment = 1,
}

pub trait HeaderParser {
    type Header;

    fn parse(reader: &mut Cursor<&[u8]>) -> Result<Self::Header>;
    fn write(&self, writer: &mut Cursor<&mut [u8]>) -> Result<()>;

    /// Header size in bytes
    fn size() -> usize;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PacketHeader {
    // protocol_id: u16,
    // crc32: u32, // append protocol_id when calculating crc32
    pub sequence: u16,
}

impl HeaderParser for PacketHeader {
    type Header = Self;

    fn size() -> usize {
        3 
    }

    fn write(&self, buffer: &mut Cursor<&mut [u8]>) -> Result<()> {
        buffer.write_u8(PacketType::Packet as u8)?;
        buffer.write_u16::<BigEndian>(self.sequence)?;
        Ok(())
    }

    fn parse(reader: &mut Cursor<&[u8]>) -> Result<Self> {
        let packet_type = reader.read_u8()?;
        if packet_type != PacketType::Packet as u8 {
            return Err(RenetError::InvalidHeaderType);
        }
        let sequence = reader.read_u16::<BigEndian>()?;

        let header = PacketHeader {
            sequence,
        };

        Ok(header)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FragmentHeader {
    // crc32: u32,
    pub sequence: u16,
    pub fragment_id: u8,
    pub num_fragments: u8,
}

impl HeaderParser for FragmentHeader {
    type Header = Self;

    fn size() -> usize {
        5 
    }

    fn write(&self, writer: &mut Cursor<&mut [u8]>) -> Result<()> {
        writer.write_u8(PacketType::Fragment as u8)?;
        writer.write_u16::<BigEndian>(self.sequence)?;
        writer.write_u8(self.fragment_id)?;
        writer.write_u8(self.num_fragments)?;
        Ok(() )
    }

    fn parse(reader: &mut Cursor<&[u8]>) -> Result<Self> {
        let packet_type = reader.read_u8()?;
        if packet_type != PacketType::Fragment as u8 {
            return Err(RenetError::InvalidHeaderType);
        }
        let sequence = reader.read_u16::<BigEndian>()?;
        let fragment_id = reader.read_u8()?;
        let num_fragments = reader.read_u8()?;

        let header = FragmentHeader {
            sequence,
            fragment_id,
            num_fragments,
        };

        Ok(header)
    }
}

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
        
        let mut buffer = vec![0u8; FragmentHeader::size()]; 
        
        let mut cursor = Cursor::new(buffer.as_mut_slice());
        fragment_header.write(&mut cursor).unwrap();

        let mut cursor = Cursor::new(buffer.as_slice());
        let parsed_fragment_header = FragmentHeader::parse(&mut cursor).unwrap(); 
        assert_eq!(fragment_header, parsed_fragment_header);
    }
    
    #[test]
    fn packet_header_read_write() {
        let header = PacketHeader {
            sequence: 42,
        };
        
        let mut buffer = vec![0u8; PacketHeader::size()]; 
        
        let mut cursor = Cursor::new(buffer.as_mut_slice());
        header.write(&mut cursor).unwrap();
        
        let mut cursor = Cursor::new(buffer.as_slice());
        let parsed_header = PacketHeader::parse(&mut cursor).unwrap(); 
        assert_eq!(header, parsed_header);
    }
}
