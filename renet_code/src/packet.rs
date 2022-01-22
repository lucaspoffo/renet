use aead::{Error as CryptoError, Tag};
use chacha20poly1305::ChaCha20Poly1305;
use std::io::Write;
use std::net::SocketAddr;
use std::{error, fmt, io};

use crate::crypto::{dencrypted, dencrypted_in_place, encrypt, encrypt_in_place};
use crate::serialize::*;
use crate::NETCODE_VERSION_INFO;

struct ConnectToken {
    version_info: [u8; 13], // "NETCODE 1.02" ASCII with null terminator.
    protocol_id: u64,       // 64 bit value unique to this particular game/application
    create_timestamp: u64,  // 64 bit unix timestamp when this connect token was created
    expire_timestamp: u64,  // 64 bit unix timestamp when this connect token expires
    connect_token_nonce: [u8; 24],
    /*
        The nonce used for encryption is a 24 bytes number that is randomly generated for every token.
        Encryption is performed on the first 1024 - 16 bytes in the buffer, leaving the last 16 bytes to store the HMAC:

            [encrypted private connect token] (1008 bytes)
            [hmac of encrypted private connect token] (16 bytes)
    */
    encrypted_private_token_data: [u8; 1024],
    timeout_seconds: u64,      // timeout in seconds. negative values disable timeout (dev only)
    num_server_addresses: u32, // in [1,32] TODO: check necessity if we use Vec
    server_addresses: [SocketAddr; 32],
    client_to_server_key: [u8; 32],
    server_to_client_key: [u8; 32],
    // <zero pad to 2048 bytes>
}

/*
    Challenge Token

    Challenge tokens stop clients with spoofed IP packet source addresses from connecting to servers.

    Prior to encryption, challenge tokens have the following structure:

        [client id] (uint64)
        [user data] (256 bytes)
        <zero pad to 300 bytes>

    Encryption of the challenge token data is performed with the libsodium AEAD primitive crypto_aead_chacha20poly1305_ietf_encrypt with no associated data, a random key generated when the dedicated server starts, and a sequence number that starts at zero and increases with each challenge token generated. The sequence number is extended by padding high bits with zero to create a 96 bit nonce.

    Encryption is performed on the first 300 - 16 bytes, and the last 16 bytes store the HMAC of the encrypted buffer:

    [encrypted challenge token] (284 bytes)
    [hmac of encrypted challenge token data] (16 bytes)
    This is referred to as the encrypted challenge token data.

*/

struct ChallengeToken {
    client_id: u64,
    user_data: [u8; 256],
    // <zero pad to 300 bytes>
}

/*
    Packets

    netcode.io has the following packets:

    connection request packet (0)
    connection denied packet (1)
    connection challenge packet (2)
    connection response packet (3)
    connection keep alive packet (4)
    connection payload packet (5)
    connection disconnect packet (6)
*/

#[repr(u8)]
enum PacketType {
    ConnectionRequest = 0,
    ConnectionDenied = 1,
    Challenge = 2,
    Response = 3,
    KeepAlive = 4,
    Payload = 5,
    Disconnect = 6,
}
impl PacketType {
    fn from_u8(value: u8) -> Result<Self, NetcodeError> {
        use PacketType::*;

        let packet_type = match value {
            0 => ConnectionRequest,
            1 => ConnectionDenied,
            2 => Challenge,
            3 => Response,
            4 => KeepAlive,
            5 => Payload,
            6 => Disconnect,
            _ => return Err(NetcodeError::InvalidPacketType),
        };
        Ok(packet_type)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    ConnectionRequest(ConnectionRequest),
    ConnectionDenied,
    Challenge(ConnectionChallenge),
    Response(ConnectionResponse),
    KeepAlive(ConnectionKeepAlive),
    Payload(usize),
    Disconnect,
}

impl Packet {
    pub fn id(&self) -> u8 {
        let packet_type = match self {
            Packet::ConnectionRequest(_) => PacketType::ConnectionRequest,
            Packet::ConnectionDenied => PacketType::ConnectionDenied,
            Packet::Challenge(_) => PacketType::Challenge,
            Packet::Response(_) => PacketType::Response,
            Packet::KeepAlive(_) => PacketType::KeepAlive,
            Packet::Payload(_) => PacketType::Payload,
            Packet::Disconnect => PacketType::Disconnect,
        };
        packet_type as u8
    }

    fn write(&self, writer: &mut impl io::Write) -> Result<(), io::Error> {
        match self {
            Packet::ConnectionRequest(ref r) => r.write(writer),
            Packet::Challenge(ref c) => c.write(writer),
            Packet::Response(ref r) => r.write(writer),
            Packet::KeepAlive(ref k) => k.write(writer),
            Packet::Payload(_) | Packet::ConnectionDenied | Packet::Disconnect => Ok(()),
        }
    }

    pub fn encode(
        &self,
        buffer: &mut [u8],
        protocol_id: u64,
        crypto_info: Option<(u64, &[u8; 32])>,
        payload: Option<&[u8]>,
    ) -> Result<(usize, Option<Tag<ChaCha20Poly1305>>), NetcodeError> {
        if let Packet::ConnectionRequest(ref request) = self {
            let mut writer = io::Cursor::new(buffer);
            let prefix_byte = encode_prefix(self.id(), 0);
            writer.write_all(&prefix_byte.to_le_bytes())?;

            request.write(&mut writer)?;
            Ok((writer.position() as usize, None))
        } else if let Some((sequence, private_key)) = crypto_info {
            let (start, end, aad) = {
                let mut writer = io::Cursor::new(&mut buffer[..]);
                let prefix_byte = {
                    let prefix_byte = encode_prefix(self.id(), sequence);
                    writer.write_all(&prefix_byte.to_le_bytes())?;
                    write_sequence(&mut writer, sequence)?;
                    prefix_byte
                };

                let start = writer.position() as usize;
                self.write(&mut writer)?;

                if let Some(payload) = payload {
                    writer.write_all(payload)?;
                }

                let additional_data = get_additional_data(prefix_byte, protocol_id)?;
                (start, writer.position() as usize, additional_data)
            };

            let tag = encrypt_in_place(&mut buffer[start..end], sequence, private_key, &aad)?;
            Ok((end, Some(tag)))
        } else {
            Err(NetcodeError::InvalidPrivateKey)
        }
    }

    pub fn decode(
        buffer: &mut [u8],
        protocol_id: u64,
        private_key: Option<&[u8; 32]>,
        tag: &Tag<ChaCha20Poly1305>,
    ) -> Result<(u64, Self), NetcodeError> {
        let src = &mut io::Cursor::new(buffer);
        let prefix_byte = read_u8(src)?;
        let (packet_type, sequence_len) = decode_prefix(prefix_byte);
        if packet_type == PacketType::ConnectionRequest as u8 {
            Ok((0, Packet::ConnectionRequest(ConnectionRequest::read(src)?)))
        } else if let Some(private_key) = private_key {
            let sequence = read_sequence(src, sequence_len)?;
            let additional_data = get_additional_data(prefix_byte, protocol_id)?;

            let pos = src.position() as usize;
            dencrypted_in_place(&mut src.get_mut()[pos..], sequence, private_key, &additional_data, tag)?;
            let packet_type = PacketType::from_u8(packet_type)?;
            let packet = match packet_type {
                PacketType::Challenge => Packet::Challenge(ConnectionChallenge::read(src)?),
                PacketType::Response => Packet::Response(ConnectionResponse::read(src)?),
                PacketType::KeepAlive => Packet::KeepAlive(ConnectionKeepAlive::read(src)?),
                PacketType::ConnectionDenied => Packet::ConnectionDenied,
                PacketType::Disconnect => Packet::Disconnect,
                PacketType::Payload => Packet::Payload(pos),
                PacketType::ConnectionRequest => unreachable!(),
            };

            Ok((sequence, packet))
        } else {
            Err(NetcodeError::InvalidPrivateKey)
        }
    }
}

fn get_additional_data(prefix: u8, protocol_id: u64) -> Result<[u8; 13 + 8 + 1], io::Error> {
    let mut buffer = [0; 13 + 8 + 1];
    buffer[..13].copy_from_slice(NETCODE_VERSION_INFO);
    buffer[13..21].copy_from_slice(&protocol_id.to_le_bytes());
    buffer[21] = prefix;

    Ok(buffer)
}

/*
    The first packet type connection request packet (0) is not encrypted and has the following format:

        0 (uint8) // prefix byte of zero
        [version info] (13 bytes)       // "NETCODE 1.02" ASCII with null terminator.
        [protocol id] (8 bytes)
        [connect token expire timestamp] (8 bytes)
        [connect token nonce] (24 bytes)
        [encrypted private connect token data] (1024 bytes)

    All other packet types are encrypted.

    Prior to encryption, packet types >= 1 have the following format:

        [prefix byte] (uint8) // non-zero prefix byte
        [sequence number] (variable length 1-8 bytes)
        [per-packet type data] (variable length according to packet type)

    The low 4 bits of the prefix byte contain the packet type.

    The high 4 bits contain the number of bytes for the sequence number in the range [1,8].

    The sequence number is encoded by omitting high zero bytes.
    For example, a sequence number of 1000 is 0x000003E8 and requires only two bytes to send its value.
    Therefore, the high 4 bits of the prefix byte are set to 2 and the sequence data written to the packet is:

        0xE8,0x03

    The sequence number bytes are reversed when written to the packet like so:

        <for each sequence byte written>
        {
            write_byte( sequence_number & 0xFF )
            sequence_number >>= 8
        }

    After the sequence number comes the per-packet type data:

    connection denied packet:

        <no data>

    connection challenge packet:

        [challenge token sequence] (uint64)
        [encrypted challenge token data] (300 bytes)

    connection response packet:

        [challenge token sequence] (uint64)
        [encrypted challenge token data] (300 bytes)

    connection keep-alive packet:

        [client index] (uint32)
        [max clients] (uint32)

    connection payload packet:

        [payload data] (1 to 1200 bytes)

    connection disconnect packet:

        <no data>

    The per-packet type data is encrypted using the libsodium AEAD primitive crypto_aead_chacha20poly1305_ietf_encrypt with the following binary data as the associated data:

        [version info] (13 bytes)       // "NETCODE 1.02" ASCII with null terminator.
        [protocol id] (uint64)          // 64 bit value unique to this particular game/application
        [prefix byte] (uint8)           // prefix byte in packet. stops an attacker from modifying packet type.

    The packet sequence number is extended by padding high bits with zero to create a 96 bit nonce.

    Packets sent from client to server are encrypted with the client to server key in the connect token.

    Packets sent from server to client are encrypted using the server to client key in the connect token for that client.

    Post encryption, packet types >= 1 have the following format:

        [prefix byte] (uint8) // non-zero prefix byte: ( (num_sequence_bytes<<4) | packet_type )
        [sequence number] (variable length 1-8 bytes)
        [encrypted per-packet type data] (variable length according to packet type)
        [hmac of encrypted per-packet type data] (16 bytes)
*/

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectionRequest {
    version_info: [u8; 13], // "NETCODE 1.02" ASCII with null terminator.
    protocol_id: u64,
    expire_timestamp: u64,
    nonce: [u8; 24],
    data: [u8; 1024],
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectionChallenge {
    token_sequence: u64,
    token_data: [u8; 300],
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectionResponse {
    token_sequence: u64,
    token_data: [u8; 300],
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectionKeepAlive {
    client_index: u32,
    max_clients: u32,
}

#[derive(Debug, PartialEq, Eq)]
struct ConnectionPayload {
    // TODO: use config for setting max payload size?
    //       netcode seems to use an define NETCODE_MAX_PAYLOAD_BYTES
    payload: Vec<u8>, // [1, 1200] bytes,
}

impl ConnectionRequest {
    fn read(src: &mut impl io::Read) -> Result<Self, io::Error> {
        let version_info = read_bytes(src)?;
        let protocol_id = read_u64(src)?;
        let token_expire_timestamp = read_u64(src)?;
        let token_nonce = read_bytes(src)?;
        let token_data = read_bytes(src)?;

        Ok(Self {
            version_info,
            protocol_id,
            expire_timestamp: token_expire_timestamp,
            nonce: token_nonce,
            data: token_data,
        })
    }

    fn write(&self, out: &mut impl io::Write) -> Result<(), io::Error> {
        out.write_all(&self.version_info)?;
        out.write_all(&self.protocol_id.to_le_bytes())?;
        out.write_all(&self.expire_timestamp.to_le_bytes())?;
        out.write_all(&self.nonce)?;
        out.write_all(&self.data)?;

        Ok(())
    }
}

impl ConnectionChallenge {
    fn read(src: &mut impl io::Read) -> Result<Self, io::Error> {
        let token_sequence: u64 = read_u64(src)?;
        let token_data = read_bytes(src)?;

        Ok(Self {
            token_sequence,
            token_data,
        })
    }

    fn write(&self, out: &mut impl io::Write) -> Result<(), io::Error> {
        out.write_all(&self.token_sequence.to_le_bytes())?;
        out.write_all(&self.token_data)?;

        Ok(())
    }
}

impl ConnectionResponse {
    fn read(src: &mut impl io::Read) -> Result<Self, io::Error> {
        let token_sequence: u64 = read_u64(src)?;
        let token_data = read_bytes(src)?;

        Ok(Self {
            token_sequence,
            token_data,
        })
    }

    fn write(&self, out: &mut impl io::Write) -> Result<(), io::Error> {
        out.write_all(&self.token_sequence.to_le_bytes())?;
        out.write_all(&self.token_data)?;

        Ok(())
    }
}

impl ConnectionKeepAlive {
    fn read(src: &mut impl io::Read) -> Result<Self, io::Error> {
        let client_index = read_u32(src)?;
        let max_clients = read_u32(src)?;

        Ok(Self { client_index, max_clients })
    }

    fn write(&self, out: &mut impl io::Write) -> Result<(), io::Error> {
        out.write_all(&self.client_index.to_le_bytes())?;
        out.write_all(&self.max_clients.to_le_bytes())?;

        Ok(())
    }
}

/*
    Reading Encrypted Packets

    The following steps are taken when reading an encrypted packet, in this exact order:

        If the packet size is less than 18 bytes then it is too small to possibly be valid, ignore the packet.

        If the low 4 bits of the prefix byte are greater than or equal to 7, the packet type is invalid, ignore the packet.

        The server ignores packets with type connection challenge packet.

        The client ignores packets with type connection request packet and connection response packet.

        If the high 4 bits of the prefix byte (sequence bytes) are outside the range [1,8], ignore the packet.

        If the packet size is less than 1 + sequence bytes + 16, it cannot possibly be valid, ignore the packet.

        If the per-packet type data size does not match the expected size for the packet type, ignore the packet.

            0 bytes for connection denied packet
            308 bytes for connection challenge packet
            308 bytes for connection response packet
            8 bytes for connection keep-alive packet
            [1,1200] bytes for connection payload packet
            0 bytes for connection disconnect packet

        If the packet type fails the replay protection already received test, ignore the packet.
        See the section on replay protection below for details.

        If the per-packet type data fails to decrypt, ignore the packet.

        Advance the most recent replay protection sequence #. See the section on replay protection below for details.

        If all the above checks pass, the packet is processed.
*/

#[derive(Debug)]
pub enum NetcodeError {
    InvalidPrivateKey,
    InvalidPacketType,
    PacketTooSmall,
    CryptoError,
    IoError(io::Error),
}

impl fmt::Display for NetcodeError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use NetcodeError::*;

        match *self {
            InvalidPrivateKey => write!(fmt, "invalid private key"),
            InvalidPacketType => write!(fmt, "invalid packet type"),
            PacketTooSmall => write!(fmt, "packet is too small"),
            CryptoError => write!(fmt, "error while encoding or decoding"),
            IoError(ref io_err) => write!(fmt, "{}", io_err),
        }
    }
}

impl error::Error for NetcodeError {}

impl From<io::Error> for NetcodeError {
    fn from(inner: io::Error) -> Self {
        NetcodeError::IoError(inner)
    }
}

impl From<CryptoError> for NetcodeError {
    fn from(_: CryptoError) -> Self {
        NetcodeError::CryptoError
    }
}

fn decode_prefix(value: u8) -> (u8, usize) {
    ((value & 0xF) as u8, (value >> 4) as usize)
}

fn encode_prefix(value: u8, sequence: u64) -> u8 {
    value | ((sequence_bytes_required(sequence) as u8) << 4)
}

fn sequence_bytes_required(sequence: u64) -> usize {
    let mut mask: u64 = 0xFF00_0000_0000_0000;
    for i in 0..8 {
        if (sequence & mask) != 0x00 {
            return 8 - i;
        }

        mask >>= 8;
    }

    0
}

fn write_sequence(out: &mut impl io::Write, seq: u64) -> Result<usize, io::Error> {
    let len = sequence_bytes_required(seq);
    let sequence_scratch = seq.to_le_bytes();
    out.write(&sequence_scratch[..len])
}

fn read_sequence(source: &mut impl io::Read, len: usize) -> Result<u64, io::Error> {
    let mut seq_scratch = [0; 8];
    source.read_exact(&mut seq_scratch[0..len])?;
    Ok(u64::from_le_bytes(seq_scratch))
}

#[cfg(test)]
mod tests {
    use crate::{NETCODE_BUFFER_SIZE, NETCODE_MAX_PACKET_SIZE};

    use super::*;

    #[test]
    fn connection_request_serialization() {
        let connection_request = ConnectionRequest {
            version_info: [0; 13], // "NETCODE 1.02" ASCII with null terminator.
            protocol_id: 1,
            expire_timestamp: 3,
            nonce: [4; 24],
            data: [5; 1024],
        };
        let mut buffer = Vec::new();
        connection_request.write(&mut buffer).unwrap();
        let deserialized = ConnectionRequest::read(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized, connection_request);
    }

    #[test]
    fn connection_challenge_serialization() {
        let connection_challenge = ConnectionChallenge {
            token_sequence: 0,
            token_data: [1u8; 300],
        };

        let mut buffer = Vec::new();
        connection_challenge.write(&mut buffer).unwrap();
        let deserialized = ConnectionChallenge::read(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized, connection_challenge);
    }

    #[test]
    fn connection_response_serialization() {
        let connection_response = ConnectionResponse {
            token_sequence: 0,
            token_data: [1u8; 300],
        };

        let mut buffer = Vec::new();
        connection_response.write(&mut buffer).unwrap();
        let deserialized = ConnectionResponse::read(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized, connection_response);
    }

    #[test]
    fn connection_keep_alive_serialization() {
        let connection_keep_alive = ConnectionKeepAlive {
            max_clients: 2,
            client_index: 1,
        };

        let mut buffer = Vec::new();
        connection_keep_alive.write(&mut buffer).unwrap();
        let deserialized = ConnectionKeepAlive::read(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized, connection_keep_alive);
    }

    #[test]
    fn test_prefix_sequence() {
        let packet_type = Packet::Disconnect.id();
        let sequence = 99999;

        let mut buffer = vec![];
        write_sequence(&mut buffer, sequence).unwrap();

        let prefix = encode_prefix(packet_type, sequence);
        let (d_packet_type, sequence_len) = decode_prefix(prefix);
        assert_eq!(packet_type, d_packet_type);
        assert_eq!(buffer.len(), sequence_len);

        let d_sequence = read_sequence(&mut buffer.as_slice(), sequence_len).unwrap();

        assert_eq!(sequence, d_sequence);
    }

    #[test]
    fn test_encrypt_decrypt_disconnect_packet() {
        let mut buffer = [0u8; NETCODE_BUFFER_SIZE];
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::Disconnect;
        let protocol_id = 12;
        let sequence = 1;
        let (len, tag) = packet.encode(&mut buffer, protocol_id, Some((sequence, key)), None).unwrap();
        let tag = tag.unwrap();
        let (d_sequence, d_packet) = Packet::decode(&mut buffer[..len], protocol_id, Some(&key), &tag).unwrap();
        assert_eq!(sequence, d_sequence);
        assert_eq!(packet, d_packet);
    }

    #[test]
    fn test_encrypt_decrypt_denied_packet() {
        let mut buffer = [0u8; NETCODE_BUFFER_SIZE];
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::ConnectionDenied;
        let protocol_id = 12;
        let sequence = 2;
        let (len, tag) = packet.encode(&mut buffer, protocol_id, Some((sequence, key)), None).unwrap();
        let tag = tag.unwrap();
        let (d_sequence, d_packet) = Packet::decode(&mut buffer[..len], protocol_id, Some(&key), &tag).unwrap();
        assert_eq!(sequence, d_sequence);
        assert_eq!(packet, d_packet);
    }

    #[test]
    fn test_encrypt_decrypt_payload_packet() {
        let mut buffer = [0u8; NETCODE_BUFFER_SIZE];
        let payload = vec![7u8; NETCODE_MAX_PACKET_SIZE];
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::Payload(2);
        let protocol_id = 12;
        let sequence = 2;
        let (len, tag) = packet
            .encode(&mut buffer, protocol_id, Some((sequence, key)), Some(&payload))
            .unwrap();
        let tag = tag.unwrap();
        let (d_sequence, d_packet) = Packet::decode(&mut buffer[..len], protocol_id, Some(&key), &tag).unwrap();
        assert_eq!(sequence, d_sequence);
        match d_packet {
            Packet::Payload(start) => assert_eq!(&payload, &buffer[start..start + NETCODE_MAX_PACKET_SIZE]),
            _ => unreachable!(),
        }
        assert_eq!(packet, d_packet);
    }
}
