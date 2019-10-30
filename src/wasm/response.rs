use std::fmt;

use bytes::Bytes;
use js_sys::Uint8Array;
use http::{HeaderMap, StatusCode};

/// A Response to a submitted `Request`.
pub struct Response {
    http: http::Response<web_sys::Response>,
}

impl Response {
    pub(super) fn new(
        res: http::Response<web_sys::Response>,
        //url: Url,
    ) -> Response {
        Response {
            http: res,
        }
    }

    /// Get the `StatusCode` of this `Response`.
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.http.status()
    }

    /// Get the `Headers` of this `Response`.
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        self.http.headers()
    }

    /// Get a mutable reference to the `Headers` of this `Response`.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        self.http.headers_mut()
    }

    /* It might not be possible to detect this in JS?
    /// Get the HTTP `Version` of this `Response`.
    #[inline]
    pub fn version(&self) -> Version {
        self.http.version()
    }
    */

    // pub async fn json()


    /// Get the response text.
    pub async fn text(self) -> crate::Result<String> {
        let p = self.http.body().text()
            .map_err(crate::error::wasm)
            .map_err(crate::error::decode)?;
        let js_val = super::promise::<wasm_bindgen::JsValue>(p)
            .await
            .map_err(crate::error::decode)?;
        if let Some(s) = js_val.as_string() {
            Ok(s)
        } else {
            Err(crate::error::decode("response.text isn't string"))
        }
    }

    /// Get the response as bytes
    pub async fn bytes(self) -> crate::Result<Bytes> {
        let p = self.http.body().array_buffer()
            .map_err(crate::error::wasm)
            .map_err(crate::error::decode)?;

        let buf_js = super::promise::<wasm_bindgen::JsValue>(p)
            .await
            .map_err(crate::error::decode)?;

        let buffer = Uint8Array::new(&buf_js);
        let mut bytes = vec![0; buffer.length() as usize];
        buffer.copy_to(&mut bytes);
        Ok(bytes.into())
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Response")
            //.field("url", self.url())
            .field("status", &self.status())
            .field("headers", self.headers())
            .finish()
    }
}
