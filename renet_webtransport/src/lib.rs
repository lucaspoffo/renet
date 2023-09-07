use js_sys::Uint8Array;
use renet::RenetClient;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{ReadableStreamDefaultReader, WebTransport, WebTransportError, WebTransportOptions};

/// This is a wrapper for ['WebTransportCloseInfo']. Because it doesn't expose the fields.
///
///
/// ['WebTransportCloseInfo']: web_sys::WebTransportCloseInfo
pub struct WebTransportCloseError {
    pub code: f64,
    pub reason: String,
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct Client {
    web_transport: WebTransport,
}

impl Client {
    pub async fn new(url: &str, options: Option<WebTransportOptions>) -> Result<Self, WebTransportError> {
        let web_transport = Self::init_web_transport(url, options).await?;
        Ok(Self { web_transport })
    }

    pub async fn update(&mut self, renet_client: &mut RenetClient) {
        let reader: ReadableStreamDefaultReader = JsValue::from(self.web_transport.datagrams().readable().get_reader()).into();
        loop {
            let (done, value): (bool, Uint8Array) = JsFuture::from(reader.read())
                .await
                .and_then(|js_value| {
                    let done = js_sys::Reflect::get(&js_value, &JsValue::from_str("done"))
                        .unwrap()
                        .as_bool()
                        .unwrap();
                    let value: Uint8Array =
                        <JsValue as Into<Uint8Array>>::into(js_sys::Reflect::get(&js_value, &JsValue::from_str("value")).unwrap());
                    Ok((done, value))
                })
                .unwrap();
            if done {
                break;
            }
            let bytes = value.to_vec();
            renet_client.process_packet(&bytes);
        }
    }

    pub async fn send_packets(&mut self, renet_client: &mut RenetClient) {
        let packets = renet_client.get_packets_to_send();
        let writer = self.web_transport.datagrams().writable().get_writer().unwrap();
        for packet in packets {
            let data = Uint8Array::new_with_length(packet.len() as u32);
            data.copy_from(&packet);
            let _ = JsFuture::from(writer.write_with_chunk(&data.into())).await;
        }
    }

    pub async fn disconnect(&mut self) -> Result<(), WebTransportCloseError> {
        self.web_transport.close();
        // once it fullies, webtransport will be closed.
        let close_result = JsFuture::from(self.web_transport.closed()).await;
        if close_result.is_err() {
            let closed_js_value = close_result.unwrap_err();
            let code = js_sys::Reflect::get(&closed_js_value, &JsValue::from_str("code"))
                .unwrap()
                .as_f64()
                .unwrap();
            let reason = js_sys::Reflect::get(&closed_js_value, &JsValue::from_str("reason"))
                .unwrap()
                .as_string()
                .unwrap();
            return Err(WebTransportCloseError { code, reason });
        }
        Ok(())
    }

    async fn init_web_transport(url: &str, options: Option<WebTransportOptions>) -> Result<WebTransport, WebTransportError> {
        let init_result = match options {
            Some(options) => WebTransport::new_with_options(&url, &options),
            None => WebTransport::new(&url),
        };
        if init_result.is_err() {
            return Err(init_result.unwrap_err().into());
        }
        let web_transport = init_result.unwrap();
        // returns undefined, once it fullies, webtransport will be ready to use.
        let _ = JsFuture::from(web_transport.ready()).await;
        Ok(web_transport)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.web_transport.close();
    }
}
