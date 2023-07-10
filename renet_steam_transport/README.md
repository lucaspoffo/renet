# Usage

Since we depend on steam we need to initialize it first. And we need to regular run the callbacks so that the transport will be updated. Examples can be find in the `demo_bevy` and `demo_chat` folder.

## Client

The Client has one extra thing, it needs to update a callback to get the current connection state of the client.

# Thanks

The code for callbacks was looked/copied up by [`bevy-steamworks`](https://github.com/HouraiTeahouse/bevy-steamworks).
