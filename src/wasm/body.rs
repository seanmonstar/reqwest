/// dox
use bytes::Bytes;
use std::fmt;
use js_sys::Uint8Array;

/// The body of a `Request`.
///
/// In most cases, this is not needed directly, as the
/// [`RequestBuilder.body`][builder] method uses `Into<Body>`, which allows
/// passing many things (like a string or vector of bytes).
///
/// [builder]: ./struct.RequestBuilder.html#method.body
pub struct Body {
    inner: Inner
}

enum Inner {
    Bytes(Bytes),
}

impl Body {
    pub(crate) fn to_js_value(&self) -> wasm_bindgen::JsValue {
        match &self.inner {
            Inner::Bytes(body_bytes) => {
                let body_bytes: &[u8] = body_bytes.as_ref();
                let body_array: Uint8Array = body_bytes.into();
                body_array.into()
            }
        }
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(bytes: Bytes) -> Body {
        Body { inner: Inner::Bytes(bytes) }
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(vec: Vec<u8>) -> Body {
        Body { inner: Inner::Bytes(vec.into()) }
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Body {
        Body { inner: Inner::Bytes(Bytes::from_static(s)) }
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        Body{ inner: Inner::Bytes(s.into()) }
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Body {
        s.as_bytes().into()
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Body").finish()
    }
}
