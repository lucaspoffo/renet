# Demo Chat

Chat application made with [egui](https://github.com/emilk/egui), you can host a chat server or connect to one as a client.

[Chat Demo.webm](https://user-images.githubusercontent.com/35241085/180664911-0baf7b35-c9d4-43ff-b793-5955060adebc.webm)

This demo also comes with `Matcher`, a web service for lobby listing, that the server register/update their state.
Clients make requests to receive the lobby list from the Matcher and can request a ConnectToken to connect to the server.

This app demonstrate how to use [renet](https://github.com/lucaspoffo/renet) and show how to handle:
- Errors
- Loading <-> Connected <-> Disconnected states
- Client self host
- Lobby web service 

You can the `Matcher` service with `cargo run` inside the `macher` folder.
Same thing for the chat application, but inside the `chat` folder.
