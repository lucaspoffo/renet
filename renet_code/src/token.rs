use std::{
    error::Error,
    fmt, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use crate::{crypto::generate_random_bytes, serialize::*, NETCODE_ADDRESS_IPV4, NETCODE_ADDRESS_IPV6};

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

#[derive(Debug, PartialEq, Eq)]
struct PrivateConnectToken {
    client_id: u64,       // globally unique identifier for an authenticated client
    timeout_seconds: i32, // timeout in seconds. negative values disable timeout (dev only)
    server_addresses: [Option<SocketAddr>; 32],
    client_to_server_key: [u8; 32],
    server_to_client_key: [u8; 32],
    user_data: [u8; 256], // user defined data specific to this protocol id
                          // <zero pad to 1024 bytes>
}

#[derive(Debug)]
enum TokenGenerationError {
    MaxHostCount,
}

impl Error for TokenGenerationError {}

impl fmt::Display for TokenGenerationError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use TokenGenerationError::*;

        match *self {
            MaxHostCount => write!(fmt, "max host count"),
        }
    }
}

impl PrivateConnectToken {
    fn generate(
        client_id: u64,
        timeout_seconds: i32,
        server_addresses: Vec<SocketAddr>,
        user_data: Option<&[u8; 256]>,
    ) -> Result<Self, TokenGenerationError> {
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
            None => generate_random_bytes(),
        };

        Ok(Self {
            client_id,
            timeout_seconds,
            server_addresses: server_addresses_arr,
            client_to_server_key,
            server_to_client_key,
            user_data,
        })
    }

    fn write(&self, writer: &mut impl io::Write) -> Result<(), io::Error> {
        writer.write_all(&self.client_id.to_le_bytes())?;
        writer.write_all(&self.timeout_seconds.to_le_bytes())?;
        let mut num_server_addresses = 0u32;
        for addr in self.server_addresses.iter() {
            if addr.is_some() {
                num_server_addresses += 1;
            } else {
                break;
            }
        }
        writer.write_all(&num_server_addresses.to_le_bytes())?;
        for i in 0..num_server_addresses as usize {
            let host = self.server_addresses[i].unwrap();
            match host {
                SocketAddr::V4(addr) => {
                    writer.write_all(&NETCODE_ADDRESS_IPV4.to_le_bytes())?;
                    for i in addr.ip().octets() {
                        writer.write_all(&i.to_le_bytes())?;
                    }
                }
                SocketAddr::V6(addr) => {
                    writer.write_all(&NETCODE_ADDRESS_IPV6.to_le_bytes())?;
                    for i in addr.ip().octets() {
                        writer.write_all(&i.to_le_bytes())?;
                    }
                }
            }
            writer.write_all(&host.port().to_le_bytes())?;
        }

        writer.write_all(&self.client_to_server_key)?;
        writer.write_all(&self.server_to_client_key)?;
        writer.write_all(&self.user_data)?;

        Ok(())
    }

    fn read(src: &mut impl io::Read) -> Result<Self, io::Error> {
        let client_id = read_u64(src)?;
        let timeout_seconds = read_i32(src)?;
        let mut server_addresses = [None; 32];
        let num_server_addresses = read_u32(src)?;
        for i in 0..num_server_addresses as usize {
            let host_type = read_u8(src)?;
            match host_type {
                NETCODE_ADDRESS_IPV4 => {
                    let mut ip = [0u8; 4];
                    src.read_exact(&mut ip)?;
                    let port = read_u16(src)?;
                    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port);
                    server_addresses[i] = Some(addr);
                }
                NETCODE_ADDRESS_IPV6 => {
                    let mut ip = [0u8; 16];
                    src.read_exact(&mut ip)?;
                    let port = read_u16(src)?;
                    let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::from(ip)), port);
                    server_addresses[i] = Some(addr);
                }
                NETCODE_ADDRESS_NONE => {} // skip
                _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Unknown ip address type")),
            }
        }

        let mut client_to_server_key = [0u8; 32];
        src.read_exact(&mut client_to_server_key)?;

        let mut server_to_client_key = [0u8; 32];
        src.read_exact(&mut server_to_client_key)?;

        let mut user_data = [0u8; 256];
        src.read_exact(&mut user_data)?;

        Ok(Self {
            client_id,
            timeout_seconds,
            server_addresses,
            client_to_server_key,
            server_to_client_key,
            user_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_connect_token_serialization() {
        let hosts: Vec<SocketAddr> = vec!["127.0.0.1:8080".parse().unwrap(), "127.0.0.2:3000".parse().unwrap()];
        let token = PrivateConnectToken::generate(1, 5, hosts, Some(&generate_random_bytes())).unwrap();
        let mut buffer: Vec<u8> = vec![];

        token.write(&mut buffer).unwrap();
        let result = PrivateConnectToken::read(&mut buffer.as_slice()).unwrap();

        assert_eq!(token, result);
    }
}
