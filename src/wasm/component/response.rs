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
    // The incoming body must be persisted if streaming to keep the stream open
    incoming_body: Option<wasi::http::types::IncomingBody>,
}

impl Response {
    pub(super) fn new(
        res: http::Response<wasi::http::types::IncomingResponse>,
        url: Url,
    ) -> Response {
        Response {
            http: res,
            url: Box::new(url),
            incoming_body: None,
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
        self.bytes()
            .await
            .map(|s| String::from_utf8(s.to_vec()).map_err(crate::error::decode))?
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

    /// Convert the response into a [`wasi::http::types::IncomingBody`] resource which can
    /// then be used to stream the body.
    #[cfg(feature = "stream")]
    pub fn bytes_stream(&mut self) -> crate::Result<wasi::io::streams::InputStream> {
        let body = self
            .http
            .body()
            .consume()
            .map_err(|_| crate::error::decode("failed to consume response body"))?;
        let stream = body
            .stream()
            .map_err(|_| crate::error::decode("failed to stream response body"));
        self.incoming_body = Some(body);
        stream
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
