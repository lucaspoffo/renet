# CHANGELOGS

## 0.0.15 - UNRELEASED

### Renet Fixed 🐛

* Now using `ClientId` from `renet_core` everywhere, in `renet` and `renetcode`

## 0.0.14 - 12-11-2023

### Renet

#### Added ⭐

* Added `is_connecting`, `is_connected` to `RenetClient`, this should make it easier to write code more transport agnostic. Also added `set_connected`, `set_connecting` so the transport layer can keep the connection status updated for the `RenetClient` (if you are using the default transport layer you will not need call it, but if you have a custom transport layer you will need to call them). [(PR)](https://github.com/lucaspoffo/renet/pull/119) by [OleStrohm](https://github.com/OleStrohm)
* Added methods `can_send_message` and `channel_available_memory` for `RenetClient` and `RenetServer`. [(commit)](https://github.com/lucaspoffo/renet/commit/edec20e4a2a9bc003375de5a7ffad4f4081c198b)
* Renetcode: make fields for ConnectToken public. [(PR)](https://github.com/lucaspoffo/renet/pull/116) by [UkoeHB](https://github.com/UkoeHB)

#### Fixed 🐛

* Fixed `RenetServer.connected_clients` not returning the correct numbers of connected clients. [(commit)](https://github.com/lucaspoffo/renet/commit/a7bced4cfb7fbe60f1447f9eb6423d2a8bc1cc6e)

#### Changed 🛠️

* Use ClientId struct instead of u64 for clients ids, better type safety for users. [(PR)](https://github.com/lucaspoffo/renet/pull/103) by [roboteng](https://github.com/roboteng)
* NetcodeServer: now accepts multiple server addresses when being created. [(PR)](https://github.com/lucaspoffo/renet/pull/102)
* NetcodeServer: change arguments when creating it, now accepts only a configuration struct `ServerConfig`, that has all previous arguments. [(PR)](https://github.com/lucaspoffo/renet/pull/102)
* RenetVisualizer: updated to egui 0.23. [(PR)](https://github.com/lucaspoffo/renet/pull/103) by [Zajozor](https://github.com/Zajozor)
* BevyRenet: updated to bevy 0.12 [(commit)](https://github.com/lucaspoffo/renet/commit/fb71a405deaf2f90c8d5f609c0e40f5a594beacf)
* BevyRenet: `client_disconnected`, `client_connecting`, `client_just_connected`, `client_just_disconnected` has been moved out of the transport module. Now they use the methods from `RenetClient` instead of the transport layer. [(PR)](https://github.com/lucaspoffo/renet/pull/119) by [OleStrohm](https://github.com/OleStrohm)

#### Removed 🔥

* Netcode: removed `is_connecting`, `is_connected`, `is_disconnected` from `NetcodeClientTransport`, use the methods from `RenetClient` instead. [(PR)](https://github.com/lucaspoffo/renet/pull/119) by [OleStrohm](https://github.com/OleStrohm)

## 0.0.13 - 19-07-2023

### Renet

#### Added ⭐

* Added missing method for retriving clients `SocketAddr` for connected clients in the default server transport: `client_addr`. Also added method to retrieve client `SocketAddr` from default client transport: `addr`. [(commit)](https://github.com/lucaspoffo/renet/commit/1ce104b547cd559fd9e8445b3fe92bb55ec19bb5)
* Add iterator access to client ids in the server: `clients_id_iter`, `disconnections_id_iter`. [(PR)](https://github.com/lucaspoffo/renet/pull/91) by [UkoeHB](https://github.com/UkoeHB)

#### Fixed 🐛

* Fix not removing items when receiving message out of order. [(commit)](https://github.com/lucaspoffo/renet/commit/3ff7dd0b7ed4cf30e2ee36a849986fc8e6e99f78)
* Correctly calculate small packet size for unreliable channels. [(commit)](https://github.com/lucaspoffo/renet/commit/6d65002e0bd1a10959b9bff6ac723ea4e55b26bf)

### Bevy Renet

#### Added ⭐

* Added new runs coditions: `client_just_connected` and `client_just_diconnected`. [(PR)](https://github.com/lucaspoffo/renet/pull/96) by [Shatur](https://github.com/Shatur)

#### Changed 🛠️

* Updated to bevy 0.11. [(PR)](https://github.com/lucaspoffo/renet/pull/93) by [Olle-Lukowski](https://github.com/Olle-Lukowski)
* Run conditions now returns closures, similar to how bevy run conditions do. [(PR)](https://github.com/lucaspoffo/renet/pull/96) by [Shatur](https://github.com/Shatur)

#### Removed 🔥

* Removed `RenetSet` and `TransportSet`. Use `resource_exists::<RenetClient>()` and `resource_exists::<RenetServer>()` instead. [(commit)](https://github.com/lucaspoffo/renet/commit/5f61be5bcd9532bd6a10192ea921d25b731549b1)

## 0.0.12 - 19-05-2023

This release comes with 3 major changes:

* Rewrite of the reliability/fragmentation protocol, it should handle any bandwidth.
* Split of renet and the transport layer. Now its possible to use others transport protocols.
* Configuration simplification

### Renet

#### Added ⭐

* Added `NetcodeServer` and `NetcodeClient` all the network transport logic has been moved to these structs.
  * You can check the documentation, examples, or demos to see how these new structs are used.

#### Changed 🛠️

* Replaced `RenetConnectionConfig` with `ConnectionConfig`, this new configuration has been greatly simplified. You only need to specify how many bytes you can use per update tick and the channels configuration (`ChannelConfiguration`).
* Rework of `ChannelConfiguration`, only 3 fields now: `channel_id`, `max_memory_usage_bytes`, and `send_type`.
* `RenetServer::new`and `RenetClient::new` now only has `ConnectionConfig` as argument.
* Renamed `RenetServer::is_client_connected` to `is_connected`.
* `NetworkInfo` has `bytes_sent_per_second` and `bytes_received_per_second` instead of `sent_kbps` and `received_kbps`.

#### Removed 🔥

* Removed `send_packets` from `RenetServer` and `RenetClient`, this is now done by the transport layer.
* Removed `can_send_message`, this could cause some bad practices of checking before sending messages. If you cannot send a message you should change the channels configurations.

### Rechannel

Rechannel has been deprecrated, all its logic has been moved inside renet.

### Bevy Renet

#### Added ⭐

* Added new Plugins for the transport layer: `NetcodeServerPlugin` and `NetcodeClientPlugin`.
* Client disconnects when the app closes.
* Server disconnects all clients when the app closes.

#### Changed 🛠️

* Removed the `clear_events` options, you can achieve the same functionality by adding the Events yourself before the Plugin.

## 0.0.11 - 12-03-2023

### Added ⭐

* BevyRenet: updated bevy to version 0.10. [(PR)](https://github.com/lucaspoffo/renet/pull/77) by [Shatur](https://github.com/Shatur)
* RenetVisualizer: updated to egui 0.21.
* Renet, Renetcode, BevyRenet: add client.is_connecting(). [(commit)](https://github.com/lucaspoffo/renet/commit/88834d4d2c9708ecec0c7f2997ca52b2b4d56641)
* Renet: allow to send empty messages again, this was a bad change since some serialization methods can serialize to empty messages. [(commit)](https://github.com/lucaspoffo/renet/commit/1e287018c7201ec339406a8dd6483714ade7f0ba)

### Changed 🛠️

* Renetcode: rename client.connected() with client.is_connected() for consistency. [(commit)](https://github.com/lucaspoffo/renet/commit/88834d4d2c9708ecec0c7f2997ca52b2b4d56641)

### Contributors 🙏

* [Shatur](https://github.com/Shatur)

## 0.0.10 - 18-11-2022

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

## 0.0.9 - 25-07-2022
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
