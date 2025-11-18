use std::fmt;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use url::Url;

use crate::{response::ResponseUrl, wasm::AbortGuard};

use super::Body;

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
        Body::from_response(self.http.into_body(), self._abort)
            .text()
            .await
    }

    /// Get the response as bytes
    pub async fn bytes(self) -> crate::Result<Bytes> {
        Body::from_response(self.http.into_body(), self._abort)
            .bytes()
            .await
    }

    /// Convert the response into a `Stream` of `Bytes` from the body.
    #[cfg(feature = "stream")]
    pub fn bytes_stream(self) -> impl futures_core::Stream<Item = crate::Result<Bytes>> {
        let body = Body::from_response(self.http.into_body(), self._abort);
        body.bytes_stream()
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

impl From<Response> for http::Response<Body> {
    fn from(response: Response) -> http::Response<Body> {
        let Response { http, _abort, url } = response;
        let (mut parts, body) = http.into_parts();
        parts.extensions.insert(ResponseUrl(*url));
        let body = Body::from_response(body, _abort);
        http::Response::from_parts(parts, body)
    }
}
