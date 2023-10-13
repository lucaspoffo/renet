use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
#[cfg(not(feature = "worker"))]
use js_sys::{Promise, Uint8Array};
use renet::RenetClient;
use send_wrapper::SendWrapper;
#[cfg(not(feature = "worker"))]
use wasm_bindgen::{prelude::Closure, JsValue};
#[cfg(not(feature = "worker"))]
use wasm_bindgen_futures::{spawn_local, JsFuture};
#[cfg(not(feature = "worker"))]
use web_sys::{ReadableStreamDefaultReader, WritableStreamDefaultWriter};

use crate::bindings::{ReadableStreamDefaultReadResult, WebTransportOptions};

use super::bindings::{WebTransport, WebTransportError};
#[cfg(feature = "worker")]
use js_sys::{Array, Promise, Uint8Array};
#[cfg(feature = "worker")]
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
#[cfg(feature = "worker")]
use wasm_bindgen_futures::JsFuture;
#[cfg(feature = "worker")]
use web_sys::{Blob, BlobPropertyBag, MessageEvent, ReadableStreamDefaultReader, Url, Worker, WritableStreamDefaultWriter};

#[cfg(feature = "worker")]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct WebTransportClient {
    web_transport: WebTransport,
    #[allow(dead_code)]
    persistent_callback_handle: Closure<dyn FnMut(MessageEvent)>,
    #[allow(dead_code)]
    close_callback_handle: Closure<dyn FnMut(JsValue)>,
    reciever: UnboundedReceiver<Vec<u8>>,
    close_reciever: UnboundedReceiver<bool>,
    worker: Worker,
    writer: WritableStreamDefaultWriter,
    is_disconnected: bool,
}

#[cfg(not(feature = "worker"))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct WebTransportClient {
    web_transport: WebTransport,
    #[allow(dead_code)]
    close_callback_handle: Closure<dyn FnMut(JsValue)>,
    reciever: UnboundedReceiver<Vec<u8>>,
    close_reciever: UnboundedReceiver<bool>,
    writer: WritableStreamDefaultWriter,
    is_disconnected: bool,
}

impl WebTransportClient {
    #[cfg(feature = "worker")]
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

        async function read() {
            while (true) {
                if(reader) {
                    const { value, done } = await reader.read();
                    if (done) {
                        break;
                    }
                    self.postMessage(value);
                } else {
                    await timer(100);
                }
            }
        }

        read();
        "
            .into(),
        );

        let blob = Blob::new_with_str_sequence_and_options(&script, BlobPropertyBag::new().type_("text/javascript"))?;
        let url = Url::create_object_url_with_blob(&blob)?;
        let worker = Worker::new(&url)?;
        let transfer_array = Array::new_with_length(1);
        transfer_array.set(0, web_transport.datagrams().readable().into());
        worker.post_message_with_transfer(&web_transport.datagrams().readable(), &transfer_array)?;
        let (sender, reciever) = unbounded::<Vec<u8>>();
        let persistent_callback_handle = Self::get_on_msg_callback(sender);
        worker.set_onmessage(Some(persistent_callback_handle.as_ref().unchecked_ref()));

        let writer = web_transport.datagrams().writable().get_writer()?;

        let (close_sender, close_reciever) = futures_channel::mpsc::unbounded::<bool>();
        let close_callback_handle: Closure<dyn FnMut(JsValue)> = Self::get_close_callback(close_sender);
        let _ = web_transport.closed().then(&close_callback_handle).catch(&close_callback_handle);

        Ok(Self {
            web_transport,
            persistent_callback_handle,
            reciever,
            worker,
            writer,
            close_reciever,
            close_callback_handle,
            is_disconnected: false,
        })
    }

    #[cfg(not(feature = "worker"))]
    pub async fn new(url: &str, options: Option<WebTransportOptions>) -> Result<Self, JsValue> {
        let web_transport = Self::init_web_transport(url, options).await?;

        let (sender, reciever) = unbounded::<Vec<u8>>();

        let reader_value = web_transport.datagrams().readable();
        spawn_local(async move {
            let reader: ReadableStreamDefaultReader = JsValue::from(reader_value.get_reader()).into();
            loop {
                let value = JsFuture::from(reader.read()).await;
                if value.is_err() {
                    break;
                }
                let result: ReadableStreamDefaultReadResult = value.unwrap().into();
                if result.is_done() {
                    break;
                }
                let data: Uint8Array = result.value().into();
                let _ = sender.unbounded_send(data.to_vec());
            }
        });

        let writer = web_transport.datagrams().writable().get_writer()?;

        let (close_sender, close_reciever) = futures_channel::mpsc::unbounded::<bool>();
        let close_callback_handle: Closure<dyn FnMut(JsValue)> = Self::get_close_callback(close_sender);
        let _ = web_transport.closed().then(&close_callback_handle).catch(&close_callback_handle);

        Ok(Self {
            web_transport,
            reciever,
            writer,
            close_reciever,
            close_callback_handle,
            is_disconnected: false,
        })
    }

    pub fn update(&mut self, renet_client: &mut RenetClient) {
        if self.is_disconnected {
            return;
        }
        if let Ok(response) = self.close_reciever.try_next() {
            self.is_disconnected = response.is_some();
        }
        while let Ok(packet) = self.reciever.try_next() {
            if let Some(packet) = packet {
                renet_client.process_packet(&packet);
            } else {
                break;
            }
        }
    }

    pub fn send_packets(&mut self, renet_client: &mut RenetClient) {
        if self.is_disconnected {
            return;
        }

        let packets = renet_client.get_packets_to_send();
        for packet in packets {
            let data = Uint8Array::new_with_length(packet.len() as u32);
            data.copy_from(&packet);
            handle_promise(self.writer.write_with_chunk(&data.into()));
        }
    }

    pub fn disconnect(self) {
        handle_promise(self.writer.close());
        self.web_transport.close();
        #[cfg(feature = "worker")]
        self.worker.terminate();
    }

    pub fn is_disconnected(&self) -> bool {
        self.is_disconnected
    }

    async fn init_web_transport(url: &str, options: Option<WebTransportOptions>) -> Result<WebTransport, WebTransportError> {
        let web_transport = match options {
            Some(options) => WebTransport::new_with_options(url, &options),
            None => WebTransport::new(url),
        }?;
        // the Promise value is undefined and discarded, but once it completes the webtransport will be ready to use.
        JsFuture::from(web_transport.ready()).await?;
        Ok(web_transport)
    }

    #[cfg(feature = "worker")]
    /// Create a closure to act on the message returned by the worker
    fn get_on_msg_callback(sender: UnboundedSender<Vec<u8>>) -> Closure<dyn FnMut(MessageEvent)> {
        Closure::new(move |event: MessageEvent| {
            let data = event.data();
            sender.unbounded_send(<JsValue as Into<Uint8Array>>::into(data).to_vec()).unwrap();
        })
    }

    /// Creates a closure to act on the close event
    fn get_close_callback(sender: UnboundedSender<bool>) -> Closure<dyn FnMut(JsValue)> {
        Closure::new(move |_| {
            sender.unbounded_send(true).unwrap();
        })
    }
}

impl Drop for WebTransportClient {
    fn drop(&mut self) {
        handle_promise(self.writer.close());
        self.web_transport.close();
        #[cfg(feature = "worker")]
        self.worker.terminate();
    }
}

unsafe impl Sync for WebTransportClient {}
unsafe impl Send for WebTransportClient {}

/// Properly handles a promise.
///
/// A promise runs in the background, but it can have side effect when not handled correctly see https://github.com/typescript-eslint/typescript-eslint/blob/main/packages/eslint-plugin/docs/rules/no-floating-promises.md
pub(crate) fn handle_promise(promise: Promise) {
    type OptionalCallback = Option<SendWrapper<Closure<dyn FnMut(JsValue)>>>;
    static mut GET_NOTHING_CALLBACK_HANDLE: OptionalCallback = None;

    let nothing_callback_handle = unsafe {
        if GET_NOTHING_CALLBACK_HANDLE.is_none() {
            let cached_callback = Closure::new(|_| {});
            GET_NOTHING_CALLBACK_HANDLE = Some(SendWrapper::new(cached_callback));
        }

        GET_NOTHING_CALLBACK_HANDLE.as_deref().unwrap()
    };

    let _ = promise.catch(nothing_callback_handle);
}
