# Demo Chat

Chat application made with [egui](https://github.com/emilk/egui), you can host a chat server or connect to one as a client.

[Chat Demo.webm](https://user-images.githubusercontent.com/35241085/180664911-0baf7b35-c9d4-43ff-b793-5955060adebc.webm)

This app demonstrate how to use [renet](https://github.com/lucaspoffo/renet) and show how to handle:

- Errors
- Loading <-> Connected <-> Disconnected states
- Client self host

You can run the application with `cargo run`. You need two running applications to test.

# Steam Demo Chat

You need to use the feature `steam_transport`.
You need to add a steam_appid.txt (likely with 480 as content) to the target folder and also the [steam-SDK](https://github.com/Noxime/steamworks-rs/tree/master/steamworks-sys/lib/steam/redistributable_bin).
You cannot run both at the same time. You will need two different devices.
If your SteamID is not yet known to you, it will be printed in the console.
