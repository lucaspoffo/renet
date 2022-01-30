# Renetcode

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
Check out the echo example to see an usage with UDP.
