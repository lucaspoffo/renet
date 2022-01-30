# Renetcode
[![Latest version](https://img.shields.io/crates/v/renetcode.svg)](https://crates.io/crates/renetcode)
[![Documentation](https://docs.rs/renetcode/badge.svg)](https://docs.rs/renetcode)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)


Renetcode is a simple connection based client/server protocol agnostic to the transport layer,
was developed be used in games with UDP in mind. Implements the Netcode 1.02 standard, available
[here][standard] and the original implementation in C++ is available in the [netcode][netcode]
repository.

Has the following feature:
- Encrypted and signed packets
- Secure client connection with connect tokens
- Connection based protocol

and protects the game server from the following attacks:
- Zombie clients
- Man in the middle
- DDoS amplification
- Packet replay attacks

[standard]: https://github.com/networkprotocol/netcode/blob/master/STANDARD.md
[netcode]: https://github.com/networkprotocol/netcode

## Usage
Check out the echo example to see an usage with UDP. Run the server with: 
```
cargo run --example echo -- server 5000 
```
run the client with:
```
cargo run --example echo -- client 5000 my_username
```