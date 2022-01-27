use aead::Error as CryptoError;
use std::io::{Cursor, Write};
use std::{error, fmt, io};

use crate::crypto::{dencrypted_in_place, encrypt_in_place};
use crate::token::{ConnectToken, TokenGenerationError};
use crate::{
    serialize::*, NETCODE_CHALLENGE_TOKEN_BYTES, NETCODE_CONNECT_TOKEN_PRIVATE_BYTES, NETCODE_CONNECT_TOKEN_XNONCE_BYTES,
    NETCODE_KEY_BYTES, NETCODE_MAC_BYTES,
};
use crate::{NETCODE_USER_DATA_BYTES, NETCODE_VERSION_INFO};

#[repr(u8)]
pub enum PacketType {
    ConnectionRequest = 0,
    ConnectionDenied = 1,
    Challenge = 2,
    Response = 3,
    KeepAlive = 4,
    Payload = 5,
    Disconnect = 6,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Packet<'a> {
    ConnectionRequest(ConnectionRequest),
    ConnectionDenied,
    Challenge(EncryptedChallengeToken),
    Response(EncryptedChallengeToken),
    KeepAlive(ConnectionKeepAlive),
    Payload(&'a [u8]),
    Disconnect,
}

// TODO: missing some fields, checkout the standard
#[derive(Debug, PartialEq, Eq)]
pub struct ConnectionRequest {
    pub version_info: [u8; 13], // "NETCODE 1.02" ASCII with null terminator.
    pub protocol_id: u64,
    pub expire_timestamp: u64,
    pub xnonce: [u8; NETCODE_CONNECT_TOKEN_XNONCE_BYTES],
    pub data: [u8; NETCODE_CONNECT_TOKEN_PRIVATE_BYTES],
}

#[derive(Debug, PartialEq, Eq)]
pub struct EncryptedChallengeToken {
    pub token_sequence: u64,
    pub token_data: [u8; NETCODE_CHALLENGE_TOKEN_BYTES], // encrypted ChallengeToken
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChallengeToken {
    pub client_id: u64,
    pub user_data: [u8; 256],
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectionKeepAlive {
    pub client_index: u32,
    pub max_clients: u32,
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

impl<'a> Packet<'a> {
    pub fn packet_type(&self) -> PacketType {
        match self {
            Packet::ConnectionRequest(_) => PacketType::ConnectionRequest,
            Packet::ConnectionDenied => PacketType::ConnectionDenied,
            Packet::Challenge(_) => PacketType::Challenge,
            Packet::Response(_) => PacketType::Response,
            Packet::KeepAlive(_) => PacketType::KeepAlive,
            Packet::Payload(_) => PacketType::Payload,
            Packet::Disconnect => PacketType::Disconnect,
        }
    }

    pub fn id(&self) -> u8 {
        self.packet_type() as u8
    }

    fn write(&self, writer: &mut impl io::Write) -> Result<(), io::Error> {
        match self {
            Packet::ConnectionRequest(ref r) => r.write(writer),
            Packet::Challenge(ref c) => c.write(writer),
            Packet::Response(ref r) => r.write(writer),
            Packet::KeepAlive(ref k) => k.write(writer),
            Packet::Payload(p) => writer.write_all(p),
            Packet::ConnectionDenied | Packet::Disconnect => Ok(()),
        }
    }

    pub fn encode(&self, buffer: &mut [u8], protocol_id: u64, crypto_info: Option<(u64, &[u8; 32])>) -> Result<usize, NetcodeError> {
        if let Packet::ConnectionRequest(ref request) = self {
            let mut writer = io::Cursor::new(buffer);
            let prefix_byte = encode_prefix(self.id(), 0);
            writer.write_all(&prefix_byte.to_le_bytes())?;

            request.write(&mut writer)?;
            Ok(writer.position() as usize)
        } else if let Some((sequence, private_key)) = crypto_info {
            let (start, end, aad) = {
                let mut writer = io::Cursor::new(&mut *buffer);
                let prefix_byte = {
                    let prefix_byte = encode_prefix(self.id(), sequence);
                    writer.write_all(&prefix_byte.to_le_bytes())?;
                    write_sequence(&mut writer, sequence)?;
                    prefix_byte
                };

                let start = writer.position() as usize;
                self.write(&mut writer)?;

                let additional_data = get_additional_data(prefix_byte, protocol_id);
                (start, writer.position() as usize, additional_data)
            };
            if buffer.len() < end + NETCODE_MAC_BYTES {
                return Err(NetcodeError::IoError(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "buffer too small to encode with encryption tag",
                )));
            }

            encrypt_in_place(&mut buffer[start..end + NETCODE_MAC_BYTES], sequence, private_key, &aad)?;
            Ok(end + NETCODE_MAC_BYTES)
        } else {
            Err(NetcodeError::InvalidPrivateKey)
        }
    }

    pub fn decode(mut buffer: &'a mut [u8], protocol_id: u64, private_key: Option<&[u8; 32]>) -> Result<(u64, Self), NetcodeError> {
        if buffer.len() < 2 + NETCODE_MAC_BYTES {
            return Err(NetcodeError::PacketTooSmall);
        }

        let prefix_byte = buffer[0];
        let (packet_type, sequence_len) = decode_prefix(prefix_byte);

        if packet_type == PacketType::ConnectionRequest as u8 {
            let src = &mut io::Cursor::new(&mut buffer);
            src.set_position(1);
            Ok((0, Packet::ConnectionRequest(ConnectionRequest::read(src)?)))
        } else if let Some(private_key) = private_key {
            let (sequence, aad, read_pos) = {
                let src = &mut io::Cursor::new(&mut buffer);
                src.set_position(1);
                let sequence = read_sequence(src, sequence_len)?;
                let additional_data = get_additional_data(prefix_byte, protocol_id);
                (sequence, additional_data, src.position() as usize)
            };

            dencrypted_in_place(&mut buffer[read_pos..], sequence, private_key, &aad)?;
            let packet_type = PacketType::from_u8(packet_type)?;
            if let PacketType::Payload = packet_type {
                return Ok((sequence, Packet::Payload(&buffer[read_pos..buffer.len() - NETCODE_MAC_BYTES])));
            }

            let src = &mut io::Cursor::new(buffer);
            src.set_position(read_pos as u64);
            let packet = match packet_type {
                PacketType::Challenge => Packet::Challenge(EncryptedChallengeToken::read(src)?),
                PacketType::Response => Packet::Response(EncryptedChallengeToken::read(src)?),
                PacketType::KeepAlive => Packet::KeepAlive(ConnectionKeepAlive::read(src)?),
                PacketType::ConnectionDenied => Packet::ConnectionDenied,
                PacketType::Disconnect => Packet::Disconnect,
                PacketType::Payload | PacketType::ConnectionRequest => unreachable!(),
            };

            Ok((sequence, packet))
        } else {
            Err(NetcodeError::InvalidPrivateKey)
        }
    }
}

fn get_additional_data(prefix: u8, protocol_id: u64) -> [u8; 13 + 8 + 1] {
    let mut buffer = [0; 13 + 8 + 1];
    buffer[..13].copy_from_slice(NETCODE_VERSION_INFO);
    buffer[13..21].copy_from_slice(&protocol_id.to_le_bytes());
    buffer[21] = prefix;

    buffer
}

impl ConnectionRequest {
    pub fn from_token(connect_token: &ConnectToken) -> Self {
        Self {
            xnonce: connect_token.xnonce,
            version_info: *NETCODE_VERSION_INFO,
            protocol_id: connect_token.protocol_id,
            expire_timestamp: connect_token.expire_timestamp,
            data: connect_token.private_data,
        }
    }

    fn read(src: &mut impl io::Read) -> Result<Self, io::Error> {
        let version_info = read_bytes(src)?;
        let protocol_id = read_u64(src)?;
        let expire_timestamp = read_u64(src)?;
        let xnonce = read_bytes(src)?;
        let token_data = read_bytes(src)?;

        Ok(Self {
            version_info,
            protocol_id,
            expire_timestamp,
            xnonce,
            data: token_data,
        })
    }

    fn write(&self, out: &mut impl io::Write) -> Result<(), io::Error> {
        out.write_all(&self.version_info)?;
        out.write_all(&self.protocol_id.to_le_bytes())?;
        out.write_all(&self.expire_timestamp.to_le_bytes())?;
        out.write_all(&self.xnonce)?;
        out.write_all(&self.data)?;

        Ok(())
    }
}

impl ChallengeToken {
    pub fn new(client_id: u64, user_data: &[u8; NETCODE_USER_DATA_BYTES]) -> Self {
        Self {
            client_id,
            user_data: *user_data,
        }
    }

    fn read(src: &mut impl io::Read) -> Result<Self, io::Error> {
        let client_id = read_u64(src)?;
        let user_data: [u8; NETCODE_USER_DATA_BYTES] = read_bytes(src)?;

        Ok(Self { client_id, user_data })
    }

    fn write(&self, out: &mut impl io::Write) -> Result<(), io::Error> {
        out.write_all(&self.client_id.to_le_bytes())?;
        out.write_all(&self.user_data)?;

        Ok(())
    }
}

impl EncryptedChallengeToken {
    pub fn generate(
        client_id: u64,
        user_data: &[u8; NETCODE_USER_DATA_BYTES],
        challenge_sequence: u64,
        challenge_key: &[u8; NETCODE_KEY_BYTES],
    ) -> Result<Self, NetcodeError> {
        let token = ChallengeToken::new(client_id, user_data);
        let mut buffer = [0u8; NETCODE_CHALLENGE_TOKEN_BYTES];
        token.write(&mut Cursor::new(&mut buffer[..]))?;
        encrypt_in_place(&mut buffer, challenge_sequence, challenge_key, b"")?;

        Ok(Self {
            token_sequence: challenge_sequence,
            token_data: buffer,
        })
    }

    pub fn decode(&self, challenge_key: &[u8; NETCODE_KEY_BYTES]) -> Result<ChallengeToken, NetcodeError> {
        let mut decoded = [0u8; NETCODE_CHALLENGE_TOKEN_BYTES];
        decoded.copy_from_slice(&self.token_data);
        dencrypted_in_place(&mut decoded, self.token_sequence, challenge_key, b"")?;

        Ok(ChallengeToken::read(&mut Cursor::new(&mut decoded))?)
    }

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

#[derive(Debug)]
pub enum NetcodeError {
    InvalidPrivateKey,
    InvalidPacketType,
    InvalidProtocolID,
    InvalidVersion,
    PacketTooSmall,
    NoMoreServers,
    Expired,
    TimedOut,
    Disconnected,
    CryptoError,
    NotInHostList,
    BufferTooSmall,
    ClientNotFound,
    IoError(io::Error),
    TokenGenerationError(TokenGenerationError),
}

impl fmt::Display for NetcodeError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use NetcodeError::*;

        match *self {
            InvalidPrivateKey => write!(fmt, "invalid private key"),
            InvalidPacketType => write!(fmt, "invalid packet type"),
            InvalidProtocolID => write!(fmt, "invalid protocol id"),
            InvalidVersion => write!(fmt, "invalid version info"),
            PacketTooSmall => write!(fmt, "packet is too small"),
            Expired => write!(fmt, "connection expired"),
            TimedOut => write!(fmt, "connection timed out"),
            Disconnected => write!(fmt, "disconnected"),
            NoMoreServers => write!(fmt, "client has no more servers to connect"),
            CryptoError => write!(fmt, "error while encoding or decoding"),
            NotInHostList => write!(fmt, "token does not contain the server address"),
            ClientNotFound => write!(fmt, "client was not found"),
            BufferTooSmall => write!(fmt, "buffer"),
            IoError(ref err) => write!(fmt, "{}", err),
            TokenGenerationError(ref err) => write!(fmt, "{}", err),
        }
    }
}

impl error::Error for NetcodeError {}

impl From<io::Error> for NetcodeError {
    fn from(inner: io::Error) -> Self {
        NetcodeError::IoError(inner)
    }
}

impl From<TokenGenerationError> for NetcodeError {
    fn from(inner: TokenGenerationError) -> Self {
        NetcodeError::TokenGenerationError(inner)
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
    use crate::{crypto::generate_random_bytes, NETCODE_BUFFER_SIZE, NETCODE_MAX_PACKET_SIZE};

    use super::*;

    #[test]
    fn connection_request_serialization() {
        let connection_request = ConnectionRequest {
            xnonce: generate_random_bytes(),
            version_info: [0; 13], // "NETCODE 1.02" ASCII with null terminator.
            protocol_id: 1,
            expire_timestamp: 3,
            data: [5; 1024],
        };
        let mut buffer = Vec::new();
        connection_request.write(&mut buffer).unwrap();
        let deserialized = ConnectionRequest::read(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized, connection_request);
    }

    #[test]
    fn connection_challenge_serialization() {
        let connection_challenge = EncryptedChallengeToken {
            token_sequence: 0,
            token_data: [1u8; 300],
        };

        let mut buffer = Vec::new();
        connection_challenge.write(&mut buffer).unwrap();
        let deserialized = EncryptedChallengeToken::read(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized, connection_challenge);
    }

    #[test]
    fn encrypted_challenge_token_serialization() {
        let connection_response = EncryptedChallengeToken {
            token_sequence: 0,
            token_data: [1u8; 300],
        };

        let mut buffer = Vec::new();
        connection_response.write(&mut buffer).unwrap();
        let deserialized = EncryptedChallengeToken::read(&mut buffer.as_slice()).unwrap();

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
    fn prefix_sequence() {
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
    fn encrypt_decrypt_disconnect_packet() {
        let mut buffer = [0u8; NETCODE_BUFFER_SIZE];
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::Disconnect;
        let protocol_id = 12;
        let sequence = 1;
        let len = packet.encode(&mut buffer, protocol_id, Some((sequence, key))).unwrap();
        let (d_sequence, d_packet) = Packet::decode(&mut buffer[..len], protocol_id, Some(&key)).unwrap();
        assert_eq!(sequence, d_sequence);
        assert_eq!(packet, d_packet);
    }

    #[test]
    fn encrypt_decrypt_denied_packet() {
        let mut buffer = [0u8; NETCODE_BUFFER_SIZE];
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::ConnectionDenied;
        let protocol_id = 12;
        let sequence = 2;
        let len = packet.encode(&mut buffer, protocol_id, Some((sequence, key))).unwrap();
        let (d_sequence, d_packet) = Packet::decode(&mut buffer[..len], protocol_id, Some(&key)).unwrap();
        assert_eq!(sequence, d_sequence);
        assert_eq!(packet, d_packet);
    }

    #[test]
    fn encrypt_decrypt_payload_packet() {
        let mut buffer = [0u8; NETCODE_BUFFER_SIZE];
        let payload = vec![7u8; NETCODE_MAX_PACKET_SIZE];
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::Payload(&payload);
        let protocol_id = 12;
        let sequence = 2;
        let len = packet.encode(&mut buffer, protocol_id, Some((sequence, key))).unwrap();
        let (d_sequence, d_packet) = Packet::decode(&mut buffer[..len], protocol_id, Some(&key)).unwrap();
        assert_eq!(sequence, d_sequence);
        match d_packet {
            Packet::Payload(ref p) => assert_eq!(&payload, p),
            _ => unreachable!(),
        }
        assert_eq!(packet, d_packet);
    }

    #[test]
    fn encrypt_decrypt_challenge_token() {
        let client_id = 0;
        let user_data = generate_random_bytes();
        let challenge_key = generate_random_bytes();
        let challenge_sequence = 1;
        let token = ChallengeToken::new(client_id, &user_data);
        let connection_challenge = EncryptedChallengeToken::generate(client_id, &user_data, challenge_sequence, &challenge_key).unwrap();

        let decoded = connection_challenge.decode(&challenge_key).unwrap();
        assert_eq!(decoded, token);
    }
}
