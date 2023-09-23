use std::{cell::RefCell, rc::Rc};

use futures_channel::mpsc::{UnboundedSender, UnboundedReceiver};
use js_sys::{Uint8Array, Array};
use renet::RenetClient;
use wasm_bindgen::{JsValue, prelude::Closure, JsCast};
use wasm_bindgen_futures::JsFuture;
pub use web_sys::WebTransportOptions;
use web_sys::{console, ReadableStreamDefaultReader, WebTransport, WebTransportError, Worker, MessageEvent};
/// This is a wrapper for ['WebTransportCloseInfo']. Because it doesn't expose the fields.
///
///
/// ['WebTransportCloseInfo']: web_sys::WebTransportCloseInfo
pub struct WebTransportCloseError {
    pub code: f64,
    pub reason: String,
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct WebTransportClient {
    web_transport: WebTransport,
    persistent_callback_handle: Closure<dyn FnMut(MessageEvent)>,
    reciever: UnboundedReceiver<Vec<u8>>,
}

impl WebTransportClient {
    pub async fn new(url: &str, options: Option<WebTransportOptions>) -> Result<Self, WebTransportError> {
        let web_transport = Self::init_web_transport(url, options).await?;

        let worker = Rc::new(RefCell::new(Worker::new("./worker.js").unwrap()));
        let worker_handle = &*worker.borrow();
        let transfer_array = Array::new_with_length(1);
        transfer_array.set(0, web_transport.datagrams().readable().get_reader().into());
        let _ = worker_handle.post_message_with_transfer(&web_transport, &transfer_array);
        let (sender, reciever) = futures_channel::mpsc::unbounded::<Vec<u8>>();
        let persistent_callback_handle = Self::get_on_msg_callback(sender);

        worker_handle.set_onmessage(Some(&persistent_callback_handle.as_ref().unchecked_ref()));

        Ok(Self { web_transport, persistent_callback_handle, reciever })
    }

    pub async fn update(&mut self, renet_client: &mut RenetClient) {
        // TODO move this into the worker thread because it blocks.
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
            break;
        }
        // TODO END

        let _ = self.reciever.try_next().map(|packet| {
            if let Some(packet) = packet {
                renet_client.process_packet(&packet);
            }
        });
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
        let web_transport = match options {
            Some(options) => WebTransport::new_with_options(&url, &options),
            None => WebTransport::new(&url),
        }?;
        // returns undefined, once it fullies, webtransport will be ready to use.
        let _ = JsFuture::from(web_transport.ready()).await;
        Ok(web_transport)
    }

    /// Create a closure to act on the message returned by the worker
    fn get_on_msg_callback(sender: UnboundedSender::<Vec<u8>>) -> Closure<dyn FnMut(MessageEvent)> {
        Closure::new(move |event: MessageEvent| {
            let data = event.data();
            sender.unbounded_send(<JsValue as Into<Uint8Array>>::into(data).to_vec()).unwrap();
        })
    }
}

impl Drop for WebTransportClient {
    fn drop(&mut self) {
        self.web_transport.close();
    }
}
