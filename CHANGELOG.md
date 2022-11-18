# Renet changelog

## 0.0.10 - 2022-11-18
### Added ⭐
* Added function `client_addr`, `user_data`, `is_client_connected`, `max_clients`, `connected_clients` for `RenetServer`, some utilities from `NetcodeServer`. [(commit)](https://github.com/lucaspoffo/renet/commit/576962e53a2e2b74f8f3c8355ae2abf706826f73) [(commit)](https://github.com/lucaspoffo/renet/commit/dff1fc5785ac2b82309b92477c90a250feb3af55)
* Renetcode/Renet: make generate_random_bytes public, this can be used to generate a random private key. [(commit)](https://github.com/lucaspoffo/renet/commit/f8509f11017e2d234c8059cc181f9644468ea87f)
* Rechannel: add `DefaultChannel` enum, usefull when using the default channel configuration. [(commit)](https://github.com/lucaspoffo/renet/commit/58311b4e7293555bd50e0c1d3cd325f7aa26d068)
* BevyRenet: add configurable event clearing. [(PR)](https://github.com/lucaspoffo/renet/pull/34)
* Rechannel: add configuration for unordered reliable channel. [(commit)](https://github.com/lucaspoffo/renet/commit/6f6ddf592650c124daca66cebf394bc79a0bbebc)
* Rechannel: add configuration for sequenced unreliable channel. [(commit)](https://github.com/lucaspoffo/renet/commit/a415a5d542aabc2c09cb5e80c30738c787e6d672)
* Renetcode: added logging. [(commit)](https://github.com/lucaspoffo/renet/commit/c963b65b66325c536d115faab31638f3ad2b5e48) [(commit)](https://github.com/lucaspoffo/renet/commit/2e41366cc5daa98edc07c7980fbb8199d0a555db)
* BevyRenet: updated bevy to version 0.9; [(PR)](https://github.com/lucaspoffo/renet/pull/55)

### Changed 🛠️
* Renetcode: remove `client_id` argument when creating an RenetClient. [(commit)](https://github.com/lucaspoffo/renet/commit/b2affb5d5659f4744faf8802c0718cc38c53f011)
* Rechannel: rename block channel to chunk channel, this makes clear what the channel does: the message is sliced in multiple chunks so it can be sent in multiple frames. Also, it is not confused with "blocking" thread/logic.
 

### Fixed 🐛
* Rechannel: when sending an empty message, this now gives an error. [(commit)](https://github.com/lucaspoffo/renet/commit/210c752c30d059408aa5765bb91749cbeae27ced)
* Rechannel: ignore already received block messages.  [(commit)](https://github.com/lucaspoffo/renet/commit/6c15cf3db5b704fdb1a88cb250aecab6971b4d0a)

### Contributors 🙏
* [Aceeri](https://github.com/Aceeri)
* [Alainx277](https://github.com/Alainx277)

## 0.0.9 - 2022-07-25
### Added ⭐
* Rechannel: added max_message_size configuration for channels. This also fixes an exploit over the block channel,
it was possible to send a SliceMessage with a high value of number of slices, this would cause the channel to allocate a lot of memory causing out of memories errors. [(commit)](https://github.com/lucaspoffo/renet/commit/774d0eeb1d05356edc29a368561e735b0eb8ab9f)
* Rechannel: add fuzzing testing for processing packets [(commit)](https://github.com/lucaspoffo/renet/commit/5d273a561ece040865fb8800177b4a213e61b868)
* Add Secure/Unsecure option when creating the server/client, easier for testing or prototyping. [(commit)](https://github.com/lucaspoffo/renet/commit/e635b819123d7c90ea7c4a59d79af7660ec0c0df)

### Changed 🛠️

* Genericify send/recv methods, use `Into<u8>` for channel id and `Into<Bytes>` for messages ([#28)](https://github.com/lucaspoffo/renet/pull/28)
* Rechannel: split channels into Sender Channels and Receiver Channels. Now server and clients can have different channels [(commit)](https://github.com/lucaspoffo/renet/commit/e76fb907052bbb51368d7630cdd6cb6e4a6c1df8)

### Fixed 🐛
* Rechannel: fix block messages not correctly getting acked [(commit)](https://github.com/lucaspoffo/renet/commit/ca39390d0aaeec943ff152000e102e4c95a3a73e)
* Rechannel: check chunk id when receiving block messages [(commit)](https://github.com/lucaspoffo/renet/commit/83f843859ff13f6dc2373a2b71169483ebdd78bf)
* Rechannel: don't initialize block messages with 0 slices. Malformed packets could crash the server/client with out of bounds accesses [(commit)](https://github.com/lucaspoffo/renet/commit/ca39390d0aaeec943ff152000e102e4c95a3a73e)
* Rechannel: don't ack fragment packet on the first received fragment, only when all fragments are received [(commit)](https://github.com/lucaspoffo/renet/commit/207091a12eb74e037af2642fec2aaa7dd62c2b28)

### Removed 🔥
* Removed `renet::ChannelNetworkInfo` [(commit)](https://github.com/lucaspoffo/renet/commit/e76fb907052bbb51368d7630cdd6cb6e4a6c1df8)

### Contributors 🙏
* [Aceeri](https://github.com/Aceeri)
* [Shatur](https://github.com/Shatur)
