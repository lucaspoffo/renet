# Renet
Renet is a network Server/Client library in rust to generate packets from aggregated messages. These messages can be:

- Reliable Ordered: multiple channels can be create, each with it's own configuration and ordering.
- Unreliable Unordered: messages that don't require any garantee of delivery or ordering.
- Block Reliable: for bigger messages, but only one can be sent at a time. 

This crate does not dependend on any transport layer, it's supposed to be used to create an reliable and fast Server/Client network. 
To see an implementation using UDP checkout [renet_udp](https://github.com/lucaspoffo/renet).
