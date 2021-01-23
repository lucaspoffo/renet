# Rust Easy Networking
Renet is a network library in rust to create Server/Client games with an simple setup but highly configurable.
Built on top of UDP, it's designed for speed in mind, usefull for games with fast gameplays such as FPS.

### Features:
  - Client and Server connection management
  - Secured connections (TODO)
  - Reliable and Ordered communication channel
  - Unreliable and Unordered communication channel
  - Network Measurement
  - Network Simulation (simulate packet loss, corruption, latency) (TODO)

## Roadmap v0.1
- Implement ChunkChannel to send bigger data as a block in a specific channel ([see](https://gafferongames.com/post/sending_large_blocks_of_data/))
- Implement Secured Channel ([protocol](https://github.com/networkprotocol/netcode/blob/master/STANDARD.md))
- Add Network Conditioner to emulate packet loss, latency, packet corruption ([example](https://github.com/amethyst/laminar/blob/master/src/net/link_conditioner.rs))
- Create open game demo to use as an better example in how to use the library and to define better APIs
- Rework error handling and add better description for the errors
- Add better logging
- Review Rust API Guidelines ([link](https://rust-lang.github.io/api-guidelines/about.html))
- Add documentation to the library
- Create simple examples such as pong, chat...
