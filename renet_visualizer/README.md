# Renet Visualizer
[![Latest version](https://img.shields.io/crates/v/renet_visualizer.svg)](https://crates.io/crates/renet_visualizer)
[![Documentation](https://docs.rs/bevy_renet/badge.svg)](https://docs.rs/renet_visualizer)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

A egui metrics visualizer for the [renet](https://github.com/lucaspoffo/renet) crate with simple usage.

### Usage

Client
```rust
let mut visualizer = RenetClientVisualizer::<200>::new(RenetVisualizerStyle::default());
// ..

loop {
    // Update Renet Client
    client.update(delta).unwrap();
    // Add metrics to the visualizer
    visualizer.add_network_info(client.network_info());

    // Draws a new egui window with the metrics
    visualizer.show_window(egui_ctx);

    // ..
}
```

Server
```rust
let mut visualizer = RenetServerVisualizer::<200>::new(RenetVisualizerStyle::default());
// ..

loop {
    // Update Renet Server
    server.update(delta).unwrap();

    // Add/Remove clients from the visualizer
    while let Some(event) = server.get_event() {
        match event {
            ServerEvent::ClientConnected(client_id, user_data) => {
                visualizer.add_client(client_id);
                // ...
            }
            ServerEvent::ClientDisconnected(client_id) => {
                visualizer.remove_client(client_id);
                // ...
            }
        }
    }

    // Add all clients metrics to the visualizer
    visualizer.update(&server);

    // Draws a new egui window with all clients metrics
    visualizer.show_window(egui_ctx);

    // ..
}
```
