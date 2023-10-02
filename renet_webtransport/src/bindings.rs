/// This module contains the bindings to the WebTransport API.
/// This is a temporary solution until the bindings are stable in the web_sys crate.
/// It was copied over from web_sys and modified so that it only contains the bindings which are used in this library.
use wasm_bindgen::prelude::*;
use web_sys::{DomException, ReadableStream, WritableStream};

#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = :: js_sys :: Object , js_name = WebTransport , typescript_type = "WebTransport")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[doc = "The `WebTransport` class."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport)"]
    pub type WebTransport;
    # [wasm_bindgen (structural , method , getter , js_class = "WebTransport" , js_name = ready)]
    #[doc = "Getter for the `ready` field of this object."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/ready)"]
    pub fn ready(this: &WebTransport) -> ::js_sys::Promise;
    # [wasm_bindgen (structural , method , getter , js_class = "WebTransport" , js_name = closed)]
    #[doc = "Getter for the `closed` field of this object."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/closed)"]
    pub fn closed(this: &WebTransport) -> ::js_sys::Promise;
    # [wasm_bindgen (structural , method , getter , js_class = "WebTransport" , js_name = datagrams)]
    #[doc = "Getter for the `datagrams` field of this object."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/datagrams)"]
    pub fn datagrams(this: &WebTransport) -> WebTransportDatagramDuplexStream;
    #[wasm_bindgen(catch, constructor, js_class = "WebTransport")]
    #[doc = "The `new WebTransport(..)` constructor, creating a new instance of `WebTransport`."]
    pub fn new(url: &str) -> Result<WebTransport, JsValue>;
    #[wasm_bindgen(catch, constructor, js_class = "WebTransport")]
    #[doc = "The `new WebTransport(..)` constructor, creating a new instance of `WebTransport`."]
    pub fn new_with_options(url: &str, options: &WebTransportOptions) -> Result<WebTransport, JsValue>;
    # [wasm_bindgen (method , structural , js_class = "WebTransport" , js_name = close)]
    #[doc = "The `close()` method."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/close)"]
    pub fn close(this: &WebTransport);
    # [wasm_bindgen (method , structural , js_class = "WebTransport" , js_name = close)]
    #[doc = "The `close()` method."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransport/close)"]
    pub fn close_with_close_info(this: &WebTransport, close_info: &WebTransportCloseInfo);
}

#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = DomException , extends = :: js_sys :: Object , js_name = WebTransportError , typescript_type = "WebTransportError")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[doc = "The `WebTransportError` class."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportError)"]
    pub type WebTransportError;
    #[cfg(feature = "WebTransportErrorSource")]
    # [wasm_bindgen (structural , method , getter , js_class = "WebTransportError" , js_name = source)]
    #[doc = "Getter for the `source` field of this object."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportError/source)"]
    pub fn source(this: &WebTransportError) -> WebTransportErrorSource;
    # [wasm_bindgen (structural , method , getter , js_class = "WebTransportError" , js_name = streamErrorCode)]
    #[doc = "Getter for the `streamErrorCode` field of this object."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportError/streamErrorCode)"]
    pub fn stream_error_code(this: &WebTransportError) -> Option<u8>;
    #[wasm_bindgen(catch, constructor, js_class = "WebTransportError")]
    #[doc = "The `new WebTransportError(..)` constructor, creating a new instance of `WebTransportError`."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportError/WebTransportError)"]
    pub fn new() -> Result<WebTransportError, JsValue>;
    #[wasm_bindgen(catch, constructor, js_class = "WebTransportError")]
    #[doc = "The `new WebTransportError(..)` constructor, creating a new instance of `WebTransportError`."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportError/WebTransportError)"]
    pub fn new_with_message(message: &str) -> Result<WebTransportError, JsValue>;
    #[cfg(feature = "WebTransportErrorOptions")]
    #[wasm_bindgen(catch, constructor, js_class = "WebTransportError")]
    #[doc = "The `new WebTransportError(..)` constructor, creating a new instance of `WebTransportError`."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportError/WebTransportError)"]
    pub fn new_with_message_and_options(message: &str, options: &WebTransportErrorOptions) -> Result<WebTransportError, JsValue>;
}

#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = :: js_sys :: Object , js_name = WebTransportOptions)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[doc = "The `WebTransportOptions` dictionary."]
    pub type WebTransportOptions;
}
#[cfg(web_sys_unstable_apis)]
impl WebTransportOptions {
    #[doc = "Construct a new `WebTransportOptions`."]
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut ret: Self = ::wasm_bindgen::JsCast::unchecked_into(::js_sys::Object::new());
        ret
    }
    #[doc = "Change the `allowPooling` field of this object."]
    pub fn allow_pooling(&mut self, val: bool) -> &mut Self {
        let r = ::js_sys::Reflect::set(self.as_ref(), &JsValue::from("allowPooling"), &JsValue::from(val));
        debug_assert!(r.is_ok(), "setting properties should never fail on our dictionary objects");
        let _ = r;
        self
    }

    #[doc = "Change the `requireUnreliable` field of this object."]
    pub fn require_unreliable(&mut self, val: bool) -> &mut Self {
        let r = ::js_sys::Reflect::set(self.as_ref(), &JsValue::from("requireUnreliable"), &JsValue::from(val));
        debug_assert!(r.is_ok(), "setting properties should never fail on our dictionary objects");
        let _ = r;
        self
    }

    #[doc = "Change the `serverCertificateHashes` field of this object."]
    pub fn server_certificate_hashes(&mut self, val: &::wasm_bindgen::JsValue) -> &mut Self {
        let r = ::js_sys::Reflect::set(self.as_ref(), &JsValue::from("serverCertificateHashes"), &JsValue::from(val));
        debug_assert!(r.is_ok(), "setting properties should never fail on our dictionary objects");
        let _ = r;
        self
    }
}
#[cfg(web_sys_unstable_apis)]
impl Default for WebTransportOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = :: js_sys :: Object , js_name = WebTransportDatagramDuplexStream , typescript_type = "WebTransportDatagramDuplexStream")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[doc = "The `WebTransportDatagramDuplexStream` class."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportDatagramDuplexStream)"]
    pub type WebTransportDatagramDuplexStream;
    # [wasm_bindgen (structural , method , getter , js_class = "WebTransportDatagramDuplexStream" , js_name = readable)]
    #[doc = "Getter for the `readable` field of this object."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportDatagramDuplexStream/readable)"]
    pub fn readable(this: &WebTransportDatagramDuplexStream) -> ReadableStream;
    # [wasm_bindgen (structural , method , getter , js_class = "WebTransportDatagramDuplexStream" , js_name = writable)]
    #[doc = "Getter for the `writable` field of this object."]
    #[doc = ""]
    #[doc = "[MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/WebTransportDatagramDuplexStream/writable)"]
    pub fn writable(this: &WebTransportDatagramDuplexStream) -> WritableStream;
}

#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = :: js_sys :: Object , js_name = WebTransportCloseInfo)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[doc = "The `WebTransportCloseInfo` dictionary."]
    #[doc = ""]
    #[doc = "*This API requires the following crate features to be activated: `WebTransportCloseInfo`*"]
    pub type WebTransportCloseInfo;
}
impl WebTransportCloseInfo {
    #[doc = "Construct a new `WebTransportCloseInfo`."]
    #[doc = ""]
    #[doc = "*This API requires the following crate features to be activated: `WebTransportCloseInfo`*"]
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut ret: Self = ::wasm_bindgen::JsCast::unchecked_into(::js_sys::Object::new());
        ret
    }
}
impl Default for WebTransportCloseInfo {
    fn default() -> Self {
        Self::new()
    }
}
