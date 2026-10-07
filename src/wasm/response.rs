use std::fmt;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use js_sys::Uint8Array;
use url::Url;

use crate::wasm::AbortGuard;

#[cfg(feature = "stream")]
use wasm_bindgen::JsCast;

#[cfg(feature = "stream")]
use futures_util::stream::{self, StreamExt};

#[cfg(feature = "json")]
use serde::de::DeserializeOwned;

/// A Response to a submitted `Request`.
pub struct Response {
    http: http::Response<web_sys::Response>,
    _abort: AbortGuard,
    // Boxed to save space (11 words to 1 word), and it's not accessed
    // frequently internally.
    url: Box<Url>,
}

impl Response {
    pub(super) fn new(
        res: http::Response<web_sys::Response>,
        url: Url,
        abort: AbortGuard,
    ) -> Response {
        Response {
            http: res,
            url: Box::new(url),
            _abort: abort,
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

    /// Get the content-length of this response, if known.
    ///
    /// Reasons it may not be known:
    ///
    /// - The server didn't send a `content-length` header.
    /// - The response is compressed and automatically decoded (thus changing
    ///   the actual decoded length).
    pub fn content_length(&self) -> Option<u64> {
        self.headers()
            .get(http::header::CONTENT_LENGTH)?
            .to_str()
            .ok()?
            .parse()
            .ok()
    }

    /// Get the final `Url` of this `Response`.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /* It might not be possible to detect this in JS?
    /// Get the HTTP `Version` of this `Response`.
    #[inline]
    pub fn version(&self) -> Version {
        self.http.version()
    }
    */

    /// Try to deserialize the response body as JSON.
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub async fn json<T: DeserializeOwned>(self) -> crate::Result<T> {
        let full = self.bytes().await?;

        serde_json::from_slice(&full).map_err(crate::error::decode)
    }

    /// Get the response text.
    pub async fn text(self) -> crate::Result<String> {
        let p = self
            .http
            .body()
            .text()
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
        let p = self
            .http
            .body()
            .array_buffer()
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

    /// Convert the response into a `Stream` of `Bytes` from the body.
    #[cfg(feature = "stream")]
    pub fn bytes_stream(self) -> impl futures_core::Stream<Item = crate::Result<Bytes>> {
        use futures_core::Stream;
        use std::pin::Pin;

        let web_response = self.http.into_body();
        let abort = self._abort;

        if let Some(body) = web_response.body() {
            let body = wasm_streams::ReadableStream::from_raw(body.unchecked_into());
            Box::pin(body.into_stream().map(move |buf_js| {
                // Keep the abort guard alive as long as this stream is.
                let _abort = &abort;
                let buffer = Uint8Array::new(
                    &buf_js
                        .map_err(crate::error::wasm)
                        .map_err(crate::error::decode)?,
                );
                let mut bytes = vec![0; buffer.length() as usize];
                buffer.copy_to(&mut bytes);
                Ok(bytes.into())
            })) as Pin<Box<dyn Stream<Item = crate::Result<Bytes>>>>
        } else {
            // If there's no body, return an empty stream.
            Box::pin(stream::empty()) as Pin<Box<dyn Stream<Item = crate::Result<Bytes>>>>
        }
    }

    // util methods

    /// Turn a response into an error if the server returned an error.
    pub fn error_for_status(self) -> crate::Result<Self> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            Err(crate::error::status_code(*self.url, status))
        } else {
            Ok(self)
        }
    }

    /// Turn a reference to a response into an error if the server returned an error.
    pub fn error_for_status_ref(&self) -> crate::Result<&Self> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            Err(crate::error::status_code(*self.url.clone(), status))
        } else {
            Ok(self)
        }
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

impl<T: Into<crate::wasm::Body>> From<http::Response<T>> for Response {
    fn from(r: http::Response<T>) -> Response {
        use crate::response::ResponseUrl;

        let (mut parts, body) = r.into_parts();
        let body = body.into();
        let body = body
            .as_bytes()
            .expect("wasm response conversion requires a byte-backed body");
        let mut body = body.to_vec();

        let web_response = if body.is_empty() {
            web_sys::Response::new().expect("failed to construct a wasm Response")
        } else {
            web_sys::Response::new_with_opt_u8_array(Some(&mut body))
                .expect("failed to construct a wasm Response")
        };

        let url = parts
            .extensions
            .remove::<ResponseUrl>()
            .unwrap_or_else(|| ResponseUrl(Url::parse("http://no.url.provided.local").unwrap()))
            .0;
        let http = http::Response::from_parts(parts, web_response);
        let abort = AbortGuard::new().expect("failed to construct an abort controller");

        Response {
            http,
            _abort: abort,
            url: Box::new(url),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Response;
    use crate::ResponseBuilderExt;
    use http::header::CONTENT_TYPE;
    use url::Url;
    use wasm_bindgen_test::*;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn from_http_response_preserves_response_parts() {
        let url = Url::parse("https://example.com/cached").unwrap();
        let response = http::Response::builder()
            .status(201)
            .header(CONTENT_TYPE, "text/plain")
            .url(url.clone())
            .body("cached response")
            .unwrap();

        let response = Response::from(response);

        assert_eq!(response.status(), 201);
        assert_eq!("text/plain", response.headers().get(CONTENT_TYPE).unwrap());
        assert_eq!(response.url(), &url);
        assert_eq!(response.text().await.unwrap(), "cached response");
    }
}
