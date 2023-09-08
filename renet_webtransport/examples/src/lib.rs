use renet::{ConnectionConfig, RenetClient};
use renet_webtransport::WebTransportClient;
use std::time::Duration;
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
pub struct ChatApplication {
    renet_client: RenetClient,
    web_transport_client: WebTransportClient,
    duration: f64,
    messages: Vec<String>,
}

#[wasm_bindgen]
impl ChatApplication {
    pub async fn new() -> Option<ChatApplication> {
        let connection_config = ConnectionConfig::default();
        let client = RenetClient::new(connection_config);

        let transport: WebTransportClient = WebTransportClient::new("https://127.0.0.1:4433", None).await.unwrap();
        Some(Self {
            renet_client: client,
            web_transport_client: transport,
            duration: 0.0,
            messages: Vec::with_capacity(20),
        })
    }

    pub async fn update(&mut self) {
        self.duration += 0.016;
        self.renet_client.update(Duration::from_secs_f64(self.duration));
        self.web_transport_client.update(&mut self.renet_client).await;
    }

    pub async fn send_packets(&mut self) {
        self.web_transport_client.send_packets(&mut self.renet_client).await;
    }

    pub async fn disconnect(&mut self) {
        let _ = self.web_transport_client.disconnect().await;
    }

    pub fn get_messages(&self) -> String {
        self.messages.join("\n")
    }
}
