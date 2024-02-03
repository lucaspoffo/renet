# Demo Bevy

Simple bevy application to demonstrates how you could replicate entities and send reliable messages as commands from the server/client using [renet](https://github.com/lucaspoffo/renet).

[Bevy Demo.webm](https://user-images.githubusercontent.com/35241085/180664609-f8c969e0-d313-45c0-9c04-8a116896d0bd.webm)

Running using the netcode transport:

- server: `cargo run --bin server --features transport`
- client: `cargo run --bin client --features transport`

You can toogle [renet_visualizer](https://github.com/lucaspoffo/renet/tree/master/renet_visualizer) with `F1` in the client.
