use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use js_sys::{Array, Uint8Array};
use renet::RenetClient;
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::bindings::WebTransportOptions;

use super::bindings::{WebTransport, WebTransportError};
use web_sys::{Blob, BlobPropertyBag, MessageEvent, Url, Worker, WritableStreamDefaultWriter};
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
    #[allow(dead_code)]
    persistent_callback_handle: Closure<dyn FnMut(MessageEvent)>,
    reciever: UnboundedReceiver<Vec<u8>>,
    worker: Worker,
    writer: WritableStreamDefaultWriter,
}

impl WebTransportClient {
    pub async fn new(url: &str, options: Option<WebTransportOptions>) -> Result<Self, JsValue> {
        let web_transport = Self::init_web_transport(url, options).await?;

        let script = Array::new();

        // we need to use the BLOB approach because we can't use the file approach as library
        script.push(
            &"
        let reader = null;

        const timer = ms => new Promise(res => setTimeout(res, ms))

        self.onmessage = function (event) {
          if (event.data) {
            reader = event.data.getReader();
          }
        };

        async function start() {
            while (true) {
                if(reader) {
                    // console.log('reading');
                    const { value, done } = await reader.read();
                    if (done) {
                        // console.log('done');
                        break;
                    }
                    // console.log(value);
                    self.postMessage(value);
                } else {
                    // don't block this thread, otherwise it cannot recive messages from the main thread.
                    await timer(100);
                }
            }
        }
        start();
        "
            .into(),
        );

        let blob = Blob::new_with_str_sequence_and_options(&script, BlobPropertyBag::new().type_("text/javascript"))?;
        let url = Url::create_object_url_with_blob(&blob)?;
        let worker = Worker::new(&url)?;
        let transfer_array = Array::new_with_length(1);
        transfer_array.set(0, web_transport.datagrams().readable().into());
        worker.post_message_with_transfer(&web_transport.datagrams().readable(), &transfer_array)?;
        let (sender, reciever) = futures_channel::mpsc::unbounded::<Vec<u8>>();
        let persistent_callback_handle = Self::get_on_msg_callback(sender);
        worker.set_onmessage(Some(persistent_callback_handle.as_ref().unchecked_ref()));

        let writer = web_transport.datagrams().writable().get_writer()?;

        Ok(Self {
            web_transport,
            persistent_callback_handle,
            reciever,
            worker,
            writer,
        })
    }

    pub fn update(&mut self, renet_client: &mut RenetClient) {
        while let Ok(packet) = self.reciever.try_next() {
            if let Some(packet) = packet {
                renet_client.process_packet(&packet);
            } else {
                break;
            }
        }
    }

    pub async fn send_packets(&mut self, renet_client: &mut RenetClient) {
        let packets = renet_client.get_packets_to_send();

        for packet in packets {
            let data = Uint8Array::new_with_length(packet.len() as u32);
            data.copy_from(&packet);
            let _ = JsFuture::from(self.writer.write_with_chunk(&data.into())).await;
        }
    }

    pub async fn disconnect(&mut self) -> Result<(), WebTransportCloseError> {
        self.web_transport.close();
        self.worker.terminate();
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
            Some(options) => WebTransport::new_with_options(url, &options),
            None => WebTransport::new(url),
        }?;
        // returns undefined, once it fullies, webtransport will be ready to use.
        let _ = JsFuture::from(web_transport.ready()).await;
        Ok(web_transport)
    }

    /// Create a closure to act on the message returned by the worker
    fn get_on_msg_callback(sender: UnboundedSender<Vec<u8>>) -> Closure<dyn FnMut(MessageEvent)> {
        Closure::new(move |event: MessageEvent| {
            let data = event.data();
            sender.unbounded_send(<JsValue as Into<Uint8Array>>::into(data).to_vec()).unwrap();
        })
    }
}

impl Drop for WebTransportClient {
    fn drop(&mut self) {
        self.web_transport.close();
        self.worker.terminate();
    }
}
