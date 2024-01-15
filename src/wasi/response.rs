use bytes::Bytes;
use encoding_rs::{Encoding, UTF_8};
use http::{Extensions, HeaderMap, StatusCode, Version};
use mime::Mime;
#[cfg(feature = "json")]
use serde::de::DeserializeOwned;
#[cfg(feature = "json")]
use serde_json;
use std::borrow::Cow;
use std::io;
use std::io::Read;
use std::net::SocketAddr;
use http::header::CONTENT_LENGTH;
use url::Url;

use crate::Body;

use super::wasi::http::*;

/// A Response to a submitted `Request`.
#[derive(Debug)]
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: Option<Body>,
    #[allow(dead_code)] incoming_response: types::IncomingResponse,
    #[allow(dead_code)] incoming_response_body: types::IncomingBody,
    url: Url,
    extensions: Extensions,
}

impl Response {
    pub(crate) fn new(status: StatusCode, headers: HeaderMap, body: Body, incoming_response: types::IncomingResponse, incoming_response_body: types::IncomingBody, url: Url) -> Response {
        Response {
            status,
            headers,
            body: Some(body),
            incoming_response,
            incoming_response_body,
            url,
            extensions: Extensions::default(),
        }
    }

    /// Get the `StatusCode` of this `Response`.
    ///
    /// # Examples
    ///
    /// Checking for general status class:
    ///
    /// ```rust
    /// # #[cfg(feature = "json")]
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let resp = reqwest::blocking::get("http://httpbin.org/get")?;
    /// if resp.status().is_success() {
    ///     println!("success!");
    /// } else if resp.status().is_server_error() {
    ///     println!("server error!");
    /// } else {
    ///     println!("Something else happened. Status: {:?}", resp.status());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Checking for specific status codes:
    ///
    /// ```rust
    /// use reqwest::blocking::Client;
    /// use reqwest::StatusCode;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let client = Client::new();
    ///
    /// let resp = client.post("http://httpbin.org/post")
    ///     .body("possibly too large")
    ///     .send()?;
    ///
    /// match resp.status() {
    ///     StatusCode::OK => println!("success!"),
    ///     StatusCode::PAYLOAD_TOO_LARGE => {
    ///         println!("Request payload is too large!");
    ///     }
    ///     s => println!("Received response status: {:?}", s),
    /// };
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the `Headers` of this `Response`.
    ///
    /// # Example
    ///
    /// Saving an etag when caching a file:
    ///
    /// ```
    /// use reqwest::blocking::Client;
    /// use reqwest::header::ETAG;
    ///
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let client = Client::new();
    ///
    /// let mut resp = client.get("http://httpbin.org/cache").send()?;
    /// if resp.status().is_success() {
    ///     if let Some(etag) = resp.headers().get(ETAG) {
    ///         std::fs::write("etag", etag.as_bytes());
    ///     }
    ///     let mut file = std::fs::File::create("file")?;
    ///     resp.copy_to(&mut file)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to the `Headers` of this `Response`.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Retrieve the cookies contained in the response.
    ///
    /// Note that invalid 'Set-Cookie' headers will be ignored.
    ///
    /// # Optional
    ///
    /// This requires the optional `cookies` feature to be enabled.
    #[cfg(feature = "cookies")]
    #[cfg_attr(docsrs, doc(cfg(feature = "cookies")))]
    pub fn cookies<'a>(&'a self) -> impl Iterator<Item=cookie::Cookie<'a>> + 'a {
        cookie::extract_response_cookies(self.headers()).filter_map(Result::ok)
    }

    /// Get the HTTP `Version` of this `Response`.
    #[inline]
    pub fn version(&self) -> Version {
        Version::HTTP_11
    }

    /// Get the final `Url` of this `Response`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let resp = reqwest::blocking::get("http://httpbin.org/redirect/1")?;
    /// assert_eq!(resp.url().as_str(), "http://httpbin.org/get");
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Get the remote address used to get this `Response`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let resp = reqwest::blocking::get("http://httpbin.org/redirect/1")?;
    /// println!("httpbin.org address: {:?}", resp.remote_addr());
    /// # Ok(())
    /// # }
    /// ```
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        None
    }

    /// Returns a reference to the associated extensions.
    pub fn extensions(&self) -> &http::Extensions {
        &self.extensions
    }

    /// Returns a mutable reference to the associated extensions.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.extensions
    }

    /// Get the content-length of the response, if it is known.
    ///
    /// Reasons it may not be known:
    ///
    /// - The server didn't send a `content-length` header.
    /// - The response is gzipped and automatically decoded (thus changing
    ///   the actual decoded length).
    pub fn content_length(&self) -> Option<u64> {
        self.headers.get(CONTENT_LENGTH)
            .and_then(|ct_len| { ct_len.to_str().ok() })
            .and_then(|ct_len| ct_len.parse().ok())
    }

    /// Try and deserialize the response body as JSON using `serde`.
    ///
    /// # Optional
    ///
    /// This requires the optional `json` feature enabled.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate reqwest;
    /// # extern crate serde;
    /// #
    /// # use reqwest::Error;
    /// # use serde::Deserialize;
    /// #
    /// // This `derive` requires the `serde` dependency.
    /// #[derive(Deserialize)]
    /// struct Ip {
    ///     origin: String,
    /// }
    ///
    /// # fn run() -> Result<(), Error> {
    /// let json: Ip = reqwest::blocking::get("http://httpbin.org/ip")?.json()?;
    /// # Ok(())
    /// # }
    /// #
    /// # fn main() { }
    /// ```
    ///
    /// # Errors
    ///
    /// This method fails whenever the response body is not in JSON format
    /// or it cannot be properly deserialized to target type `T`. For more
    /// details please see [`serde_json::from_reader`].
    ///
    /// [`serde_json::from_reader`]: https://docs.serde.rs/serde_json/fn.from_reader.html
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn json<T: DeserializeOwned>(self) -> crate::Result<T> {
        let full = self.bytes()?;
        serde_json::from_slice(&full).map_err(crate::error::decode)
    }

    /// Get the full response body as `Bytes`.
    ///
    /// # Example
    ///
    /// ```
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let bytes = reqwest::blocking::get("http://httpbin.org/ip")?.bytes()?;
    ///
    /// println!("bytes: {:?}", bytes);
    /// # Ok(())
    /// # }
    /// ```
    pub fn bytes(mut self) -> crate::Result<Bytes> {
        let bytes: Bytes = Bytes::copy_from_slice(self.body.take().unwrap().buffer()?);
        Ok(bytes)
    }

    /// Get the response text.
    ///
    /// This method decodes the response body with BOM sniffing
    /// and with malformed sequences replaced with the REPLACEMENT CHARACTER.
    /// Encoding is determined from the `charset` parameter of `Content-Type` header,
    /// and defaults to `utf-8` if not presented.
    ///
    /// # Example
    ///
    /// ```rust
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let content = reqwest::blocking::get("http://httpbin.org/range/26")?.text()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn text(self) -> crate::Result<String> {
        self.text_with_charset("utf-8")
    }

    /// Get the response text given a specific encoding.
    ///
    /// This method decodes the response body with BOM sniffing
    /// and with malformed sequences replaced with the REPLACEMENT CHARACTER.
    /// You can provide a default encoding for decoding the raw message, while the
    /// `charset` parameter of `Content-Type` header is still prioritized. For more information
    /// about the possible encoding name, please go to [`encoding_rs`] docs.
    ///
    /// [`encoding_rs`]: https://docs.rs/encoding_rs/0.8/encoding_rs/#relationship-with-windows-code-pages
    ///
    /// # Example
    ///
    /// ```rust
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let content = reqwest::blocking::get("http://httpbin.org/range/26")?
    ///     .text_with_charset("utf-8")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn text_with_charset(self, default_encoding: &str) -> crate::Result<String> {
        let content_type = self
            .headers()
            .get(crate::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Mime>().ok());
        let encoding_name = content_type
            .as_ref()
            .and_then(|mime| mime.get_param("charset").map(|charset| charset.as_str()))
            .unwrap_or(default_encoding);
        let encoding = Encoding::for_label(encoding_name.as_bytes()).unwrap_or(UTF_8);

        let full = self.bytes()?;

        let (text, _, _) = encoding.decode(&full);
        if let Cow::Owned(s) = text {
            return Ok(s);
        }
        unsafe {
            // decoding returned Cow::Borrowed, meaning these bytes
            // are already valid utf8
            Ok(String::from_utf8_unchecked(full.to_vec()))
        }
    }

    /// Copy the response body into a writer.
    ///
    /// This function internally uses [`std::io::copy`] and hence will continuously read data from
    /// the body and then write it into writer in a streaming fashion until EOF is met.
    ///
    /// On success, the total number of bytes that were copied to `writer` is returned.
    ///
    /// [`std::io::copy`]: https://doc.rust-lang.org/std/io/fn.copy.html
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let mut resp = reqwest::blocking::get("http://httpbin.org/range/5")?;
    /// let mut buf: Vec<u8> = vec![];
    /// resp.copy_to(&mut buf)?;
    /// assert_eq!(b"abcde", buf.as_slice());
    /// # Ok(())
    /// # }
    /// ```
    pub fn copy_to<W: ?Sized>(&mut self, w: &mut W) -> crate::Result<u64>
        where
            W: io::Write,
    {
        io::copy(self, w).map_err(crate::error::decode_io)
    }

    /// Turn a response into an error if the server returned an error.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let res = reqwest::blocking::get("http://httpbin.org/status/400")?
    ///     .error_for_status();
    /// if let Err(err) = res {
    ///     assert_eq!(err.status(), Some(reqwest::StatusCode::BAD_REQUEST));
    /// }
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn error_for_status(self) -> crate::Result<Self> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            Err(crate::error::status_code(self.url.clone(), status))
        } else {
            Ok(self)
        }
    }

    /// Turn a reference to a response into an error if the server returned an error.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let res = reqwest::blocking::get("http://httpbin.org/status/400")?;
    /// let res = res.error_for_status_ref();
    /// if let Err(err) = res {
    ///     assert_eq!(err.status(), Some(reqwest::StatusCode::BAD_REQUEST));
    /// }
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn error_for_status_ref(&self) -> crate::Result<&Self> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            Err(crate::error::status_code(self.url.clone(), status))
        } else {
            Ok(self)
        }
    }
}


impl Read for Response {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.body.take().unwrap().into_reader().read(buf)
    }
}
