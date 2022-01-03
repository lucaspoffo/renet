use aead::{Aead, Error as CryptoError, NewAead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_chacha::{rand_core::{RngCore, SeedableRng}, ChaCha20Rng};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::{error, fmt};

const NETCODE_VERSION_INFO: &[u8; 13] = b"NETCODE 1.02\0";

/*
   Connect Token
   A connect token ensures that only authenticated clients can connect to dedicated servers.

   The connect token has two parts: public and private.

   The private portion is encrypted and signed with a private key shared between the web backend and dedicated server instances.

   Prior to encryption the private connect token data has the following binary format:

       [client id] (uint64) // globally unique identifier for an authenticated client
       [timeout seconds] (uint32) // timeout in seconds. negative values disable timeout (dev only)
       [num server addresses] (uint32) // in [1,32]
       <for each server address>
       {
           [address type] (uint8) // value of 1 = IPv4 address, 2 = IPv6 address.
           <if IPV4 address>
           {
               // for a given IPv4 address: a.b.c.d:port
               [a] (uint8)
               [b] (uint8)
               [c] (uint8)
               [d] (uint8)
               [port] (uint16)
           }
           <else IPv6 address>
           {
               // for a given IPv6 address: [a:b:c:d:e:f:g:h]:port
               [a] (uint16)
               [b] (uint16)
               [c] (uint16)
               [d] (uint16)
               [e] (uint16)
               [f] (uint16)
               [g] (uint16)
               [h] (uint16)
               [port] (uint16)
           }
       }
       [client to server key] (32 bytes)
       [server to client key] (32 bytes)
       [user data] (256 bytes) // user defined data specific to this protocol id
       <zero pad to 1024 bytes>

   This data is variable size but for simplicity is written to a fixed size buffer of 1024 bytes. Unused bytes are zero padded.
*/

struct PrivateConnectToken {
    client_id: u64,       // globally unique identifier for an authenticated client
    timeout_seconds: i32, // timeout in seconds. negative values disable timeout (dev only)
    server_addresses: [Option<SocketAddr>; 32],
    client_to_server_key: [u8; 32],
    server_to_client_key: [u8; 32],
    user_data: [u8; 256], // user defined data specific to this protocol id
                          // <zero pad to 1024 bytes>
}

enum TokenGenerationError {
    MaxHostCount,
}

impl PrivateConnectToken {
    fn generate(client_id: u64, timeout_seconds: i32, server_addresses: Vec<SocketAddr>, user_data: Option<&[u8; 256]>) -> Result<Self, TokenGenerationError> {
        if server_addresses.len() > 32 {
            return Err(TokenGenerationError::MaxHostCount);
        }
        let mut server_addresses_arr = [None; 32];
        for (i, addr) in server_addresses.into_iter().enumerate() {
            server_addresses_arr[i] = Some(addr);
        }

        let client_to_server_key = generate_random_bytes();
        let server_to_client_key = generate_random_bytes();

        let user_data = match user_data {
            Some(data) => *data,
            None => generate_random_bytes()
        };

        Ok(Self {
            client_id,
            timeout_seconds,
            server_addresses: server_addresses_arr,
            client_to_server_key,
            server_to_client_key,
            user_data
        })
    }
}


fn generate_random_bytes<const N: usize>() -> [u8; N] {
    let mut rng = ChaCha20Rng::from_entropy();
    let mut bytes = [0; N];
    rng.fill_bytes(&mut bytes);
    bytes
}

/*
     Encryption of the private connect token data is performed with the libsodium AEAD primitive crypto_aead_xchacha20poly1305_ietf_encrypt using the following binary data as the associated data:

       [version info] (13 bytes)       // "NETCODE 1.02" ASCII with null terminator.
        [protocol id] (uint64)          // 64 bit value unique to this particular game/application
        [expire timestamp] (uint64)     // 64 bit unix timestamp when this connect token expires

   The nonce used for encryption is a 24 bytes number that is randomly generated for every token.

    Encryption is performed on the first 1024 - 16 bytes in the buffer, leaving the last 16 bytes to store the HMAC:

    [encrypted private connect token] (1008 bytes)
    [hmac of encrypted private connect token] (16 bytes)
    Post encryption, this is referred to as the encrypted private connect token data.

    Together the public and private data form a connect token:

        [version info] (13 bytes)       // "NETCODE 1.02" ASCII with null terminator.
        [protocol id] (uint64)          // 64 bit value unique to this particular game/application
        [create timestamp] (uint64)     // 64 bit unix timestamp when this connect token was created
        [expire timestamp] (uint64)     // 64 bit unix timestamp when this connect token expires
        [connect token nonce] (24 bytes)
        [encrypted private connect token data] (1024 bytes)
        [timeout seconds] (uint32)      // timeout in seconds. negative values disable timeout (dev only)
        [num_server_addresses] (uint32) // in [1,32]
        <for each server address>
        {
            [address_type] (uint8) // value of 1 = IPv4 address, 2 = IPv6 address.
            <if IPV4 address>
            {
                // for a given IPv4 address: a.b.c.d:port
                [a] (uint8)
                [b] (uint8)
                [c] (uint8)
                [d] (uint8)
                [port] (uint16)
            }
            <else IPv6 address>
            {
                // for a given IPv6 address: [a:b:c:d:e:f:g:h]:port
                [a] (uint16)
                [b] (uint16)
                [c] (uint16)
                [d] (uint16)
                [e] (uint16)
                [f] (uint16)
                [g] (uint16)
                [h] (uint16)
                [port] (uint16)
            }
        }
        [client to server key] (32 bytes)
        [server to client key] (32 bytes)
        <zero pad to 2048 bytes>

    This data is variable size but for simplicity is written to a fixed size buffer of 2048 bytes. Unused bytes are zero padded.
*/

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
enum Packet {
    ConnectionRequest(ConnectionRequest),
    ConnectionDenied,
    Challenge(ConnectionChallenge),
    Response(ConnectionResponse),
    KeepAlive(ConnectionKeepAlive),
    Payload(Vec<u8>),
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
            Packet::Payload(p) => writer.write_all(p),
            Packet::ConnectionDenied => Ok(()),
            Packet::Disconnect => Ok(()),
        }
    }

    pub fn encode(&self, protocol_id: u64, crypto_info: Option<(u64, &[u8; 32])>) -> Result<Vec<u8>, NetcodeError> {
        if let Packet::ConnectionRequest(ref request) = self {
            let prefix_byte = encode_prefix(self.id(), 0);
            let mut buffer = vec![];
            buffer.write_all(&prefix_byte.to_le_bytes())?;

            request.write(&mut buffer)?;
            Ok(buffer)
        } else if let Some((sequence, private_key)) = crypto_info {
            let mut buffer = vec![];
            let prefix_byte = {
                let prefix_byte = encode_prefix(self.id(), sequence);
                buffer.write_all(&prefix_byte.to_le_bytes())?;
                write_sequence(&mut buffer, sequence)?;
                prefix_byte
            };

            let mut scratch = vec![];
            self.write(&mut scratch)?;

            let additional_data = get_additional_data(prefix_byte, protocol_id)?;
            println!("scratch_len: {}", scratch.len());

            dbg!(prefix_byte);
            dbg!(protocol_id);
            dbg!(additional_data);
            dbg!(private_key);

            let encrypted = encrypt(&scratch, sequence, private_key, &additional_data)?;
            buffer.write_all(&encrypted)?;
            Ok(buffer)
        } else {
            Err(NetcodeError::InvalidPrivateKey)
        }
    }

    pub fn decode(data: &[u8], protocol_id: u64, private_key: Option<&[u8; 32]>) -> Result<(u64, Self), NetcodeError> {
        let src = &mut io::Cursor::new(data);
        let prefix_byte = read_u8(src)?;
        let (packet_type, sequence_len) = decode_prefix(prefix_byte);
        if packet_type == PacketType::ConnectionRequest as u8 {
            Ok((0, Packet::ConnectionRequest(ConnectionRequest::read(src)?)))
        } else if let Some(private_key) = private_key {
            let sequence = read_sequence(src, sequence_len)?;

            let payload = &data[src.position() as usize..];

            let additional_data = get_additional_data(prefix_byte, protocol_id)?;

            dbg!(prefix_byte);
            dbg!(protocol_id);
            dbg!(additional_data);
            dbg!(private_key);

            let decrypted = dencrypted(payload, sequence, private_key, &additional_data)?;
            let packet_type = PacketType::from_u8(packet_type)?;
            let packet = match packet_type {
                PacketType::Challenge => Packet::Challenge(ConnectionChallenge::read(&mut decrypted.as_slice())?),
                PacketType::Response => Packet::Response(ConnectionResponse::read(&mut decrypted.as_slice())?),
                PacketType::KeepAlive => Packet::KeepAlive(ConnectionKeepAlive::read(&mut decrypted.as_slice())?),
                PacketType::ConnectionDenied => Packet::ConnectionDenied,
                PacketType::Disconnect => Packet::Disconnect,
                PacketType::Payload => Packet::Payload(decrypted),
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
struct ConnectionRequest {
    version_info: [u8; 13], // "NETCODE 1.02" ASCII with null terminator.
    protocol_id: u64,
    expire_timestamp: u64,
    nonce: [u8; 24],
    data: [u8; 1024],
}

#[derive(Debug, PartialEq, Eq)]
struct ConnectionChallenge {
    token_sequence: u64,
    token_data: [u8; 300],
}

#[derive(Debug, PartialEq, Eq)]
struct ConnectionResponse {
    token_sequence: u64,
    token_data: [u8; 300],
}

#[derive(Debug, PartialEq, Eq)]
struct ConnectionKeepAlive {
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

#[inline]
fn read_u64(src: &mut impl io::Read) -> Result<u64, io::Error> {
    let mut buffer = [0u8; 8];
    src.read_exact(&mut buffer)?;
    Ok(u64::from_le_bytes(buffer))
}

#[inline]
fn read_u32(src: &mut impl io::Read) -> Result<u32, io::Error> {
    let mut buffer = [0u8; 4];
    src.read_exact(&mut buffer)?;
    Ok(u32::from_le_bytes(buffer))
}

#[inline]
fn read_u8(src: &mut impl io::Read) -> Result<u8, io::Error> {
    let mut buffer = [0u8; 1];
    src.read_exact(&mut buffer)?;
    Ok(u8::from_le_bytes(buffer))
}

#[inline]
fn read_bytes<const N: usize>(src: &mut impl io::Read) -> Result<[u8; N], io::Error> {
    let mut data = [0u8; N];
    src.read_exact(&mut data)?;
    Ok(data)
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
enum NetcodeError {
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

// TODO: use decrypt_in_place
fn dencrypted(packet: &[u8], sequence: u64, private_key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(private_key);
    let cipher = ChaCha20Poly1305::new(key);
    let payload = Payload { msg: packet, aad };

    cipher.decrypt(&nonce, payload)
}

// TODO: use encrypt_in_place
fn encrypt(packet: &[u8], sequence: u64, key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0; 12];
    nonce[4..12].copy_from_slice(&sequence.to_le_bytes());
    let nonce = Nonce::from(nonce);

    let key = Key::from_slice(key);
    let cipher = ChaCha20Poly1305::new(key);
    dbg!(packet);
    let payload = Payload { msg: packet, aad };

    cipher.encrypt(&nonce, payload)
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
    fn test_encrypt_decrypt() {
        let key = b"an example very very secret key."; // 32-bytes
        let sequence = 2;
        let aad = b"test";

        let data = b"some packet data";

        let encrypted = encrypt(data, sequence, &key, aad).unwrap();
        let decrypted = dencrypted(&encrypted, sequence, &key, aad).unwrap();
        assert_eq!(data.to_vec(), decrypted);
    }

    /*
    #[test]
    fn test_encrypt_decrypt_connection_request() {
        let key = b"an example very very secret key."; // 32-bytes
        let protocol_id = 12;
        let packet = Packet::ConnectionRequest(ConnectionRequest {
            protocol_id,
            nonce,
            data,
            expire_timestamp,
            version_info
        });
        let sequence = 2;
        let encoded = packet.encode(protocol_id, Some((sequence, key))).unwrap();
        println!("encoded_len: {:?}", encoded.len());
        let (d_sequence, d_packet) = Packet::decode(&encoded, protocol_id, Some(&key)).unwrap();
        assert_eq!(sequence, d_sequence);
        assert_eq!(packet, d_packet);
    }
    */

    #[test]
    fn test_encrypt_decrypt_disconnect_packet() {
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::Disconnect;
        let protocol_id = 12;
        let sequence = 1;
        let encoded = packet.encode(protocol_id, Some((sequence, key))).unwrap();
        println!("encoded_len: {:?}", encoded.len());
        let (d_sequence, d_packet) = Packet::decode(&encoded, protocol_id, Some(&key)).unwrap();
        assert_eq!(sequence, d_sequence);
        assert_eq!(packet, d_packet);
    }

    #[test]
    fn test_encrypt_decrypt_denied_packet() {
        let key = b"an example very very secret key."; // 32-bytes
        let packet = Packet::ConnectionDenied;
        let protocol_id = 12;
        let sequence = 2;
        let encoded = packet.encode(protocol_id, Some((sequence, key))).unwrap();
        println!("encoded_len: {:?}", encoded.len());
        let (d_sequence, d_packet) = Packet::decode(&encoded, protocol_id, Some(&key)).unwrap();
        assert_eq!(sequence, d_sequence);
        assert_eq!(packet, d_packet);
    }
}
