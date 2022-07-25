# Renet changelog

## Unreleased
### Added ⭐
* Rechannel: added max_message_size configuration for channels. This fixes an exploit over the block channel,
it was possible to send a SliceMessage with a high value of num_slices, this would cause the channel to allocate a lot of memory causing out of memories errors. [(commit)](https://github.com/lucaspoffo/renet/commit/774d0eeb1d05356edc29a368561e735b0eb8ab9f)
* Rechannel: add fuzzing testing for processing packets [(commit)](https://github.com/lucaspoffo/renet/commit/5d273a561ece040865fb8800177b4a213e61b868)
* Rechannel: split channels into Sender and Receiver [(commit)](https://github.com/lucaspoffo/renet/commit/e76fb907052bbb51368d7630cdd6cb6e4a6c1df8)


### Changed 🛠️

### Fixed 🐛
* Rechannel: fix block messages not correctly getting acked [(commit)](https://github.com/lucaspoffo/renet/commit/ca39390d0aaeec943ff152000e102e4c95a3a73e)
* Rechannel: check chunk id when receiving block messages [(commit)](https://github.com/lucaspoffo/renet/commit/83f843859ff13f6dc2373a2b71169483ebdd78bf)
* Rechannel: don't initialize block messages with 0 slices. Malformed packets could cause out of bounds accesses [(commit)](https://github.com/lucaspoffo/renet/commit/ca39390d0aaeec943ff152000e102e4c95a3a73e)
* Rechannel: don't ack fragment packet on the first received fragment, only when all fragments are received [(commit)](https://github.com/lucaspoffo/renet/commit/207091a12eb74e037af2642fec2aaa7dd62c2b28)


### Contributors 🙏
