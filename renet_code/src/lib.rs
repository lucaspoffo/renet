mod client;
mod crypto;
mod packet;
mod serialize;
mod server;
mod token;

use std::time::Duration;

const NETCODE_VERSION_INFO: &[u8; 13] = b"NETCODE 1.02\0";

const NETCODE_ADDRESS_NONE: u8 = 0;
const NETCODE_ADDRESS_IPV4: u8 = 1;
const NETCODE_ADDRESS_IPV6: u8 = 2;

const NETCODE_CONNECT_TOKEN_PRIVATE_BYTES: usize = 1024;
const NETCODE_MAX_PACKET_SIZE: usize = 1200;

const NETCODE_KEY_BYTES: usize = 32;
const NETCODE_MAC_BYTES: usize = 16;
const NETCODE_USER_DATA_BYTES: usize = 256;
const NETCODE_CHALLENGE_TOKEN_BYTES: usize = 300;

const NETCODE_ADDITIONAL_DATA_SIZE: usize = 13 + 8 + 8;

const NETCODE_BUFFER_SIZE: usize = NETCODE_MAX_PACKET_SIZE + NETCODE_MAC_BYTES;
const NETCODE_TIMEOUT_SECONDS: i32 = 15;

const NETCODE_SEND_RATE: Duration = Duration::from_millis(100);
