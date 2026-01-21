#[cfg(feature = "multipart")]
use super::multipart::Form;
use super::AbortGuard;
/// dox
use bytes::Bytes;
#[cfg(feature = "stream")]
use futures_core::Stream;
#[cfg(feature = "stream")]
use futures_util::stream::{self, StreamExt};
use js_sys::Uint8Array;
#[cfg(feature = "stream")]
use std::pin::Pin;
use std::{borrow::Cow, fmt};
#[cfg(feature = "stream")]
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::Response as WebResponse;

/// The body of a `Request`.
///
/// In most cases, this is not needed directly, as the
/// [`RequestBuilder.body`][builder] method uses `Into<Body>`, which allows
/// passing many things (like a string or vector of bytes).
///
/// [builder]: ./struct.RequestBuilder.html#method.body
pub struct Body {
    inner: Inner,
}

enum Inner {
    Single(Single),
    /// MultipartForm holds a multipart/form-data body.
    #[cfg(feature = "multipart")]
    MultipartForm(Form),
    Streaming(StreamingBody),
}

#[derive(Clone)]
pub(crate) enum Single {
    Bytes(Bytes),
    Text(Cow<'static, str>),
}

impl Single {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Single::Bytes(bytes) => bytes.as_ref(),
            Single::Text(text) => text.as_bytes(),
        }
    }

    pub(crate) fn to_js_value(&self) -> JsValue {
        match self {
            Single::Bytes(bytes) => {
                let body_bytes: &[u8] = bytes.as_ref();
                let body_uint8_array: Uint8Array = body_bytes.into();
                let js_value: &JsValue = body_uint8_array.as_ref();
                js_value.to_owned()
            }
            Single::Text(text) => JsValue::from_str(text),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Single::Bytes(bytes) => bytes.is_empty(),
            Single::Text(text) => text.is_empty(),
        }
    }
}

struct StreamingBody {
    response: WebResponse,
    abort: AbortGuard,
}

impl StreamingBody {
    #[cfg(feature = "stream")]
    fn into_stream(self) -> Pin<Box<dyn Stream<Item = crate::Result<Bytes>>>> {
        let StreamingBody { response, abort } = self;

        if let Some(body) = response.body() {
            let abort = abort;
            let body = wasm_streams::ReadableStream::from_raw(body.unchecked_into());
            Box::pin(body.into_stream().map(move |buf_js| {
                // Keep the abort guard alive while the stream is active.
                let _abort = &abort;
                let buf_js = buf_js
                    .map_err(crate::error::wasm)
                    .map_err(crate::error::decode)?;
                let buffer = Uint8Array::new(&buf_js);
                let mut bytes = vec![0; buffer.length() as usize];
                buffer.copy_to(&mut bytes);
                Ok(bytes.into())
            }))
        } else {
            drop(abort);
            Box::pin(stream::empty())
        }
    }

    async fn into_bytes(self) -> crate::Result<Bytes> {
        let StreamingBody { response, abort } = self;
        let promise = response
            .array_buffer()
            .map_err(crate::error::wasm)
            .map_err(crate::error::decode)?;
        let js_value = super::promise::<wasm_bindgen::JsValue>(promise)
            .await
            .map_err(crate::error::decode)?;
        drop(abort);
        let buffer = Uint8Array::new(&js_value);
        let mut bytes = vec![0; buffer.length() as usize];
        buffer.copy_to(&mut bytes);
        Ok(bytes.into())
    }

    async fn into_text(self) -> crate::Result<String> {
        let StreamingBody { response, abort } = self;
        let promise = response
            .text()
            .map_err(crate::error::wasm)
            .map_err(crate::error::decode)?;
        let js_value = super::promise::<wasm_bindgen::JsValue>(promise)
            .await
            .map_err(crate::error::decode)?;
        drop(abort);
        if let Some(text) = js_value.as_string() {
            Ok(text)
        } else {
            Err(crate::error::decode("response.text isn't string"))
        }
    }
}

impl Body {
    /// Returns a reference to the internal data of the `Body`.
    ///
    /// `None` is returned, if the underlying data is a multipart form.
    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.inner {
            Inner::Single(single) => Some(single.as_bytes()),
            #[cfg(feature = "multipart")]
            Inner::MultipartForm(_) => None,
            Inner::Streaming(_) => None,
        }
    }

    pub(crate) fn to_js_value(&self) -> crate::Result<JsValue> {
        match &self.inner {
            Inner::Single(single) => Ok(single.to_js_value()),
            #[cfg(feature = "multipart")]
            Inner::MultipartForm(form) => {
                let form_data = form.to_form_data()?;
                let js_value: &JsValue = form_data.as_ref();
                Ok(js_value.to_owned())
            }
            Inner::Streaming(_) => Err(crate::error::decode(
                "streaming body cannot be converted to JsValue",
            )),
        }
    }

    #[cfg(feature = "multipart")]
    pub(crate) fn as_single(&self) -> Option<&Single> {
        match &self.inner {
            Inner::Single(single) => Some(single),
            Inner::MultipartForm(_) => None,
        }
    }

    #[inline]
    #[cfg(feature = "multipart")]
    pub(crate) fn from_form(f: Form) -> Body {
        Self {
            inner: Inner::MultipartForm(f),
        }
    }

    /// into_part turns a regular body into the body of a multipart/form-data part.
    #[cfg(feature = "multipart")]
    pub(crate) fn into_part(self) -> Body {
        match self.inner {
            Inner::Single(single) => Self {
                inner: Inner::Single(single),
            },
            Inner::MultipartForm(form) => Self {
                inner: Inner::MultipartForm(form),
            },
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match &self.inner {
            Inner::Single(single) => single.is_empty(),
            #[cfg(feature = "multipart")]
            Inner::MultipartForm(form) => form.is_empty(),
            Inner::Streaming(_) => false,
        }
    }

    pub(crate) fn try_clone(&self) -> Option<Body> {
        match &self.inner {
            Inner::Single(single) => Some(Self {
                inner: Inner::Single(single.clone()),
            }),
            #[cfg(feature = "multipart")]
            Inner::MultipartForm(_) => None,
            Inner::Streaming(_) => None,
        }
    }

    pub(super) fn from_response(response: WebResponse, abort: AbortGuard) -> Body {
        if response.body().is_some() {
            Body {
                inner: Inner::Streaming(StreamingBody { response, abort }),
            }
        } else {
            // Even without a body, ensure the guard lives until completion.
            drop(abort);
            Body::default()
        }
    }

    /// Consume the body into bytes.
    pub async fn bytes(self) -> crate::Result<Bytes> {
        match self.inner {
            Inner::Single(Single::Bytes(bytes)) => Ok(bytes),
            Inner::Single(Single::Text(text)) => Ok(Bytes::copy_from_slice(text.as_bytes())),
            #[cfg(feature = "multipart")]
            Inner::MultipartForm(_) => Err(crate::error::decode(
                "multipart body cannot be converted into bytes",
            )),
            Inner::Streaming(streaming) => streaming.into_bytes().await,
        }
    }

    /// Consume the body into a UTF-8 string.
    pub async fn text(self) -> crate::Result<String> {
        match self.inner {
            Inner::Single(Single::Bytes(bytes)) => String::from_utf8(bytes.to_vec())
                .map_err(|_| crate::error::decode("body is not valid UTF-8")),
            Inner::Single(Single::Text(text)) => Ok(text.into_owned()),
            #[cfg(feature = "multipart")]
            Inner::MultipartForm(_) => Err(crate::error::decode(
                "multipart body cannot be converted into text",
            )),
            Inner::Streaming(streaming) => streaming.into_text().await,
        }
    }

    /// Convert the body into a stream of `Bytes`.
    #[cfg(feature = "stream")]
    #[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
    pub fn bytes_stream(self) -> Pin<Box<dyn Stream<Item = crate::Result<Bytes>>>> {
        match self.inner {
            Inner::Single(single) => {
                let bytes = Bytes::copy_from_slice(single.as_bytes());
                Box::pin(stream::once(async move { Ok(bytes) }))
            }
            #[cfg(feature = "multipart")]
            Inner::MultipartForm(_) => Box::pin(stream::once(async {
                Err(crate::error::decode("multipart body cannot be streamed"))
            })),
            Inner::Streaming(streaming) => streaming.into_stream(),
        }
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(bytes: Bytes) -> Body {
        Body {
            inner: Inner::Single(Single::Bytes(bytes)),
        }
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(vec: Vec<u8>) -> Body {
        Body {
            inner: Inner::Single(Single::Bytes(vec.into())),
        }
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Body {
        Body {
            inner: Inner::Single(Single::Bytes(Bytes::from_static(s))),
        }
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        Body {
            inner: Inner::Single(Single::Text(s.into())),
        }
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Body {
        Body {
            inner: Inner::Single(Single::Text(s.into())),
        }
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Body").finish()
    }
}

impl Default for Body {
    fn default() -> Body {
        Body {
            inner: Inner::Single(Single::Bytes(Bytes::new())),
        }
    }
}

// Can use new methods in web-sys when requiring v0.2.93.
// > `init.method(m)` to `init.set_method(m)`
// For now, ignore their deprecation.
#[allow(deprecated)]
#[cfg(test)]
mod tests {
    use crate::Body;
    use js_sys::Uint8Array;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen]
    extern "C" {
        // Use `js_namespace` here to bind `console.log(..)` instead of just
        // `log(..)`
        #[wasm_bindgen(js_namespace = console)]
        fn log(s: String);
    }

    #[wasm_bindgen_test]
    async fn test_body() {
        let body = Body::from("TEST");
        assert_eq!([84, 69, 83, 84], body.as_bytes().unwrap());
    }

    #[wasm_bindgen_test]
    async fn test_body_js_static_str() {
        let body_value = "TEST";
        let body = Body::from(body_value);

        let mut init = web_sys::RequestInit::new();
        init.method("POST");
        init.body(Some(
            body.to_js_value()
                .expect("could not convert body to JsValue")
                .as_ref(),
        ));

        let js_req = web_sys::Request::new_with_str_and_init("", &init)
            .expect("could not create JS request");
        let text_promise = js_req.text().expect("could not get text promise");
        let text = crate::wasm::promise::<JsValue>(text_promise)
            .await
            .expect("could not get request body as text");

        assert_eq!(text.as_string().expect("text is not a string"), body_value);
    }
    #[wasm_bindgen_test]
    async fn test_body_js_string() {
        let body_value = "TEST".to_string();
        let body = Body::from(body_value.clone());

        let mut init = web_sys::RequestInit::new();
        init.method("POST");
        init.body(Some(
            body.to_js_value()
                .expect("could not convert body to JsValue")
                .as_ref(),
        ));

        let js_req = web_sys::Request::new_with_str_and_init("", &init)
            .expect("could not create JS request");
        let text_promise = js_req.text().expect("could not get text promise");
        let text = crate::wasm::promise::<JsValue>(text_promise)
            .await
            .expect("could not get request body as text");

        assert_eq!(text.as_string().expect("text is not a string"), body_value);
    }

    #[wasm_bindgen_test]
    async fn test_body_js_static_u8_slice() {
        let body_value: &'static [u8] = b"\x00\x42";
        let body = Body::from(body_value);

        let mut init = web_sys::RequestInit::new();
        init.method("POST");
        init.body(Some(
            body.to_js_value()
                .expect("could not convert body to JsValue")
                .as_ref(),
        ));

        let js_req = web_sys::Request::new_with_str_and_init("", &init)
            .expect("could not create JS request");

        let array_buffer_promise = js_req
            .array_buffer()
            .expect("could not get array_buffer promise");
        let array_buffer = crate::wasm::promise::<JsValue>(array_buffer_promise)
            .await
            .expect("could not get request body as array buffer");

        let v = Uint8Array::new(&array_buffer).to_vec();

        assert_eq!(v, body_value);
    }

    #[wasm_bindgen_test]
    async fn test_body_js_vec_u8() {
        let body_value = vec![0u8, 42];
        let body = Body::from(body_value.clone());

        let mut init = web_sys::RequestInit::new();
        init.method("POST");
        init.body(Some(
            body.to_js_value()
                .expect("could not convert body to JsValue")
                .as_ref(),
        ));

        let js_req = web_sys::Request::new_with_str_and_init("", &init)
            .expect("could not create JS request");

        let array_buffer_promise = js_req
            .array_buffer()
            .expect("could not get array_buffer promise");
        let array_buffer = crate::wasm::promise::<JsValue>(array_buffer_promise)
            .await
            .expect("could not get request body as array buffer");

        let v = Uint8Array::new(&array_buffer).to_vec();

        assert_eq!(v, body_value);
    }
}
