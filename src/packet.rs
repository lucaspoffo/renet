use super::error::{RenetError, Result};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PacketType {
    Packet = 0,
    Fragment = 1,
}

// TODO: implement prefix byte to accomodate 4 bits for the packet type
// and 4 bits for the ack_bits enconding optimization
// TODO: we can delta encode the sequence with the ack, but should we? 
pub trait HeaderParser {
    type Header;

    fn parse(reader: &[u8]) -> Result<Self::Header>;
    fn write(&self, writer: &mut [u8]) -> Result<()>;

    /// Header size in bytes
    fn size(&self) -> usize;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PacketHeader {
    // protocol_id: u16,
    // crc32: u32, // append protocol_id when calculating crc32
    pub sequence: u16,
    pub ack: u16,
    pub ack_bits: u32,
}

impl HeaderParser for PacketHeader {
    type Header = Self;

    fn size(&self) -> usize {
        9
    }

    fn write(&self, mut buffer: &mut [u8]) -> Result<()> {
        buffer.write_u8(PacketType::Packet as u8)?;
        buffer.write_u16::<BigEndian>(self.sequence)?;
        buffer.write_u16::<BigEndian>(self.ack)?;
        buffer.write_u32::<BigEndian>(self.ack_bits)?;
        Ok(())
    }

    fn parse(mut reader: &[u8]) -> Result<Self> {
        let packet_type = reader.read_u8()?;
        if packet_type != PacketType::Packet as u8 {
            return Err(RenetError::InvalidHeaderType);
        }
        let sequence = reader.read_u16::<BigEndian>()?;
        let ack = reader.read_u16::<BigEndian>()?;
        let ack_bits = reader.read_u32::<BigEndian>()?;

        let header = PacketHeader {
            sequence,
            ack,
            ack_bits,
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
    // Only the first fragment has the PacketHeader
    pub packet_header: Option<PacketHeader>,
}

impl HeaderParser for FragmentHeader {
    type Header = Self;

    fn size(&self) -> usize {
        if self.fragment_id == 0 {
            12
        } else {
            5
        }
    }

    fn write(&self, mut writer: &mut [u8]) -> Result<()> {
        writer.write_u8(PacketType::Fragment as u8)?;
        writer.write_u8(self.fragment_id)?;
        writer.write_u8(self.num_fragments)?;

        if self.fragment_id == 0 {
            if let Some(ref packet_header) = self.packet_header {
                packet_header.write(writer)?;
            } else {
                return Err(RenetError::FragmentMissingPacketHeader);
            }
        } else {
            writer.write_u16::<BigEndian>(self.sequence)?;
        }

        Ok(())
    }

    fn parse(mut reader: &[u8]) -> Result<Self> {
        let packet_type = reader.read_u8()?;
        if packet_type != PacketType::Fragment as u8 {
            return Err(RenetError::InvalidHeaderType);
        }
        let fragment_id = reader.read_u8()?;
        let num_fragments = reader.read_u8()?;

        let mut packet_header = None;
        let sequence;
        if fragment_id == 0 {
            let header = PacketHeader::parse(reader)?;
            sequence = header.sequence;
            packet_header = Some(header);
        } else {
            sequence = reader.read_u16::<BigEndian>()?;
        }

        let header = FragmentHeader {
            sequence,
            fragment_id,
            num_fragments,
            packet_header,
        };

        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_header_read_write() {
        let fragment_header = FragmentHeader {
            sequence: 42,
            fragment_id: 3,
            num_fragments: 5,
            packet_header: None
        };

        let mut buffer = vec![0u8; fragment_header.size()];

        fragment_header.write(&mut buffer).unwrap();

        let parsed_fragment_header = FragmentHeader::parse(&mut buffer).unwrap();
        assert_eq!(fragment_header, parsed_fragment_header);
    }

    #[test]
    fn packet_header_read_write() {
        let header = PacketHeader { sequence: 42, ack: 0, ack_bits: 0 };

        let mut buffer = vec![0u8; header.size()];

        header.write(&mut buffer).unwrap();

        let parsed_header = PacketHeader::parse(&mut buffer).unwrap();
        assert_eq!(header, parsed_header);
    }
}
