# Demo Bevy

Simple bevy application to demonstrates how you could replicate entities and send reliable messages as commands from the server/client using [renet](https://github.com/lucaspoffo/renet).

[Bevy Demo.webm](https://user-images.githubusercontent.com/35241085/180664609-f8c969e0-d313-45c0-9c04-8a116896d0bd.webm)

## How to run

Running using the netcode transport:

- server: `cargo run --bin server --features netcode`
- client: `cargo run --bin client --features netcode`

Running using the steam transport:

- server: `cargo run --bin server --features steam`
- client: `cargo run --bin client --features steam -- [HOST_STEAM_ID]`
  - The `HOST_STEAM_ID` is printed in the console when the server is started

## Controls

Client:

- AWSD for movement
- Mouse to AIM, left button to shoot
- F1 to toggle [renet_visualizer](https://github.com/lucaspoffo/renet/tree/master/renet_visualizer)

Server:

- SPACE to spawn a bot that shoots a lot of bullets (stress test)
