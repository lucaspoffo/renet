# Rechannel
[![Latest version](https://img.shields.io/crates/v/rechannel.svg)](https://crates.io/crates/rechannel)
[![Documentation](https://docs.rs/rechannel/badge.svg)](https://docs.rs/rechannel)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

Rechannel is a network Server/Client library in rust to send/receive messages with different channels configurations. These channels can be:

- Reliable Ordered: garantee ordering and delivery of all messages
- Unreliable Unordered: messages that don't require any garantee of delivery or ordering
- Block Reliable: for bigger messages, such as level initialization
