use std::{fmt, io::Read as _};

use bytes::Bytes;
use http::{HeaderMap, StatusCode, Version};
use url::Url;

#[cfg(feature = "json")]
use serde::de::DeserializeOwned;

/// A Response to a submitted `Request`.
pub struct Response {
    http: http::Response<wasi::http::types::IncomingResponse>,
    // Boxed to save space (11 words to 1 word), and it's not accessed
    // frequently internally.
    url: Box<Url>,
}

impl Response {
    pub(super) fn new(
        res: http::Response<wasi::http::types::IncomingResponse>,
        url: Url,
    ) -> Response {
        Response {
            http: res,
            url: Box::new(url),
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
    ///  the actual decoded length).
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

    /// Get the HTTP `Version` of this `Response`.
    #[inline]
    pub fn version(&self) -> Version {
        self.http.version()
    }

    /// Try to deserialize the response body as JSON.
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub async fn json<T: DeserializeOwned>(self) -> crate::Result<T> {
        let full = self.bytes().await?;

        serde_json::from_slice(&full).map_err(crate::error::decode)
    }

    /// Get the response text.
    pub async fn text(self) -> crate::Result<String> {
        // let p = self
        //     .http
        //     .body()
        //     .text()
        //     .map_err(crate::error::wasm)
        //     .map_err(crate::error::decode)?;
        // let js_val = super::promise::<wasm_bindgen::JsValue>(p)
        //     .await
        //     .map_err(crate::error::decode)?;
        // if let Some(s) = js_val.as_string() {
        //     Ok(s)
        // } else {
        //     Err(crate::error::decode("response.text isn't string"))
        // }
        Ok("str_resp".to_string())
    }

    /// Get the response as bytes
    pub async fn bytes(self) -> crate::Result<Bytes> {
        let response_body = self
            .http
            .body()
            .consume()
            .map_err(|_| crate::error::decode("failed to consume response body"))?;
        let body = {
            let mut buf = vec![];
            let mut stream = response_body
                .stream()
                .map_err(|_| crate::error::decode("failed to stream response body"))?;
            InputStreamReader::from(&mut stream)
                .read_to_end(&mut buf)
                .map_err(crate::error::decode_io)?;
            buf
        };
        let _trailers = wasi::http::types::IncomingBody::finish(response_body);
        Ok(body.into())
    }

    /// Convert the response into a `Stream` of `Bytes` from the body.
    #[cfg(feature = "stream")]
    pub fn bytes_stream(self) -> impl futures_core::Stream<Item = crate::Result<Bytes>> {
        let web_response = self.http.into_body();
        let abort = self._abort;
        let body = web_response
            .body()
            .expect("could not create wasm byte stream");
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
        }))
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

/// Implements `std::io::Read` for a `wasi::io::streams::InputStream`.
pub struct InputStreamReader<'a> {
    stream: &'a mut wasi::io::streams::InputStream,
}

impl<'a> From<&'a mut wasi::io::streams::InputStream> for InputStreamReader<'a> {
    fn from(stream: &'a mut wasi::io::streams::InputStream) -> Self {
        Self { stream }
    }
}

impl std::io::Read for InputStreamReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io;
        use wasi::io::streams::StreamError;

        let n = buf
            .len()
            .try_into()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        match self.stream.blocking_read(n) {
            Ok(chunk) => {
                let n = chunk.len();
                if n > buf.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "more bytes read than requested",
                    ));
                }
                buf[..n].copy_from_slice(&chunk);
                Ok(n)
            }
            Err(StreamError::Closed) => Ok(0),
            Err(StreamError::LastOperationFailed(e)) => {
                Err(io::Error::new(io::ErrorKind::Other, e.to_debug_string()))
            }
        }
    }
}
