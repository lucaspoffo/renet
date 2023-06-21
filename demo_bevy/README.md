# Demo Bevy

Simple bevy application to demonstrates how you could replicate entities and send reliable messages as commands from the server/client using [renet](https://github.com/lucaspoffo/renet).

[Bevy Demo.webm](https://user-images.githubusercontent.com/35241085/180664609-f8c969e0-d313-45c0-9c04-8a116896d0bd.webm)

To run this demo you need first to run the server with `cargo run --bin server`, then you can execute the client with `cargo run --bin client`.
You can toogle [renet_visualizer](https://github.com/lucaspoffo/renet/tree/master/renet_visualizer) with `F1` in the client.

# Demo Bevy Steam

Same as above, but you need to need to add a steam_appid.txt (likely with 480 as content) to the target folder and also the [steam-SDK](https://github.com/Noxime/steamworks-rs/tree/master/steamworks-sys/lib/steam/redistributable_bin). And insead of server its steam_server, same for client is steam_client. You cannot start both of them at the same time, this is cause of Steam, you will need two different devices.
Also you need to set the const SteamId to which you want to connect in the steam_client. Your SteamId if its not known to you will be printed in the console.