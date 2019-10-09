use std::mem;
use std::fmt;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::time::Duration;

use futures::{Async, Poll, Stream};
use http;
use serde::de::DeserializeOwned;

use cookie;
use client::KeepCoreThreadAlive;
use hyper::header::HeaderMap;
use {async_impl, StatusCode, Url, Version, wait};

/// A Response to a submitted `Request`.
pub struct Response {
    inner: async_impl::Response,
    body: Option<async_impl::ReadableChunks<WaitBody>>,
    timeout: Option<Duration>,
    _thread_handle: KeepCoreThreadAlive,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl Response {
    pub(crate) fn new(res: async_impl::Response, timeout: Option<Duration>, thread: KeepCoreThreadAlive) -> Response {
        Response {
            inner: res,
            body: None,
            timeout,
            _thread_handle: thread,
        }
    }

    /// Get the `StatusCode` of this `Response`.
    ///
    /// # Examples
    ///
    /// Checking for general status class:
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let resp = reqwest::get("http://httpbin.org/get")?;
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
    /// use reqwest::Client;
    /// use reqwest::StatusCode;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
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
        self.inner.status()
    }

    /// Get the `Headers` of this `Response`.
    ///
    /// # Example
    ///
    /// Saving an etag when caching a file:
    ///
    /// ```
    /// use reqwest::Client;
    /// use reqwest::header::ETAG;
    ///
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
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
        self.inner.headers()
    }

    /// Retrieve the cookies contained in the response.
    ///
    /// Note that invalid 'Set-Cookie' headers will be ignored.
    pub fn cookies<'a>(&'a self) -> impl Iterator< Item = cookie::Cookie<'a> > + 'a {
        cookie::extract_response_cookies(self.headers())
            .filter_map(Result::ok)
    }


    /// Get the HTTP `Version` of this `Response`.
    #[inline]
    pub fn version(&self) -> Version {
        self.inner.version()
    }

    /// Get the final `Url` of this `Response`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let resp = reqwest::get("http://httpbin.org/redirect/1")?;
    /// assert_eq!(resp.url().as_str(), "http://httpbin.org/get");
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn url(&self) -> &Url {
        self.inner.url()
    }

    /// Get the remote address used to get this `Response`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let resp = reqwest::get("http://httpbin.org/redirect/1")?;
    /// println!("httpbin.org address: {:?}", resp.remote_addr());
    /// # Ok(())
    /// # }
    /// ```
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.inner.remote_addr()
    }

    /// Get the content-length of the response, if it is known.
    ///
    /// Reasons it may not be known:
    ///
    /// - The server didn't send a `content-length` header.
    /// - The response is gzipped and automatically decoded (thus changing
    ///   the actual decoded length).
    pub fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    /// Try and deserialize the response body as JSON using `serde`.
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
    /// #[derive(Deserialize)]
    /// struct Ip {
    ///     origin: String,
    /// }
    ///
    /// # fn run() -> Result<(), Error> {
    /// let json: Ip = reqwest::get("http://httpbin.org/ip")?.json()?;
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
    /// [`serde_json::from_reader`]: https://docs.serde.rs/serde_json/fn.from_reader.html
    #[inline]
    pub fn json<T: DeserializeOwned>(&mut self) -> ::Result<T> {
        wait::timeout(self.inner.json(), self.timeout).map_err(|e| {
            match e {
                wait::Waited::TimedOut => ::error::timedout(None),
                wait::Waited::Inner(e) => e,
            }
        })
    }

    /// Get the response text.
    ///
    /// This method decodes the response body with BOM sniffing
    /// and with malformed sequences replaced with the REPLACEMENT CHARACTER.
    /// Encoding is determinated from the `charset` parameter of `Content-Type` header,
    /// and defaults to `utf-8` if not presented.
    ///
    /// # Example
    ///
    /// ```rust
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let content = reqwest::get("http://httpbin.org/range/26")?.text()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Note
    ///
    /// This consumes the body. Trying to read more, or use of `response.json()`
    /// will return empty values.
    pub fn text(&mut self) -> ::Result<String> {
        self.text_with_charset("utf-8")
    }

    /// Get the response text given a specific encoding.
    ///
    /// This method decodes the response body with BOM sniffing
    /// and with malformed sequences replaced with the REPLACEMENT CHARACTER.
    /// You can provide a default encoding for decoding the raw message, while the
    /// `charset` parameter of `Content-Type` header is still prioritized. For more information
    /// about the possible encoding name, please go to
    /// https://docs.rs/encoding_rs/0.8.17/encoding_rs/#relationship-with-windows-code-pages
    ///
    /// # Example
    ///
    /// ```rust
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let content = reqwest::get("http://httpbin.org/range/26")?.text_with_charset("utf-8")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Note
    ///
    /// This consumes the body. Trying to read more, or use of `response.json()`
    /// will return empty values.
    pub fn text_with_charset(&mut self, default_encoding: &str) -> ::Result<String> {
        wait::timeout(self.inner.text_with_charset(default_encoding), self.timeout).map_err(|e| {
            match e {
                wait::Waited::TimedOut => ::error::timedout(None),
                wait::Waited::Inner(e) => e,
            }
        })
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
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let mut resp = reqwest::get("http://httpbin.org/range/5")?;
    /// let mut buf: Vec<u8> = vec![];
    /// resp.copy_to(&mut buf)?;
    /// assert_eq!(b"abcde", buf.as_slice());
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn copy_to<W: ?Sized>(&mut self, w: &mut W) -> ::Result<u64>
        where W: io::Write
    {
        io::copy(self, w).map_err(::error::from)
    }

    /// Turn a response into an error if the server returned an error.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let res = reqwest::get("http://httpbin.org/status/400")?
    ///     .error_for_status();
    /// if let Err(err) = res {
    ///     assert_eq!(err.status(), Some(reqwest::StatusCode::BAD_REQUEST));
    /// }
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    #[inline]
    pub fn error_for_status(self) -> ::Result<Self> {
        let Response { body, inner, timeout, _thread_handle } = self;
        inner.error_for_status().map(move |inner| {
            Response {
                inner,
                body,
                timeout,
                _thread_handle,
            }
        })
    }

    /// Turn a reference to a response into an error if the server returned an error.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let res = reqwest::get("http://httpbin.org/status/400")?;
    /// let res = res.error_for_status_ref();
    /// if let Err(err) = res {
    ///     assert_eq!(err.status(), Some(reqwest::StatusCode::BAD_REQUEST));
    /// }
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    #[inline]
    pub fn error_for_status_ref(&self) -> ::Result<&Self> {
        self.inner.error_for_status_ref().and_then(|_| Ok(self))
    }
}

impl Read for Response {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.body.is_none() {
            let body = mem::replace(self.inner.body_mut(), async_impl::Decoder::empty());
            let body = async_impl::ReadableChunks::new(WaitBody {
                inner: wait::stream(body, self.timeout)
            });
            self.body = Some(body);
        }
        let mut body = self.body.take().unwrap();
        let bytes = body.read(buf);
        self.body = Some(body);
        bytes
    }
}

struct WaitBody {
    inner: wait::WaitStream<async_impl::Decoder>
}

impl Stream for WaitBody {
    type Item = <async_impl::Decoder as Stream>::Item;
    type Error = <async_impl::Decoder as Stream>::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.next() {
            Some(Ok(chunk)) => Ok(Async::Ready(Some(chunk))),
            Some(Err(e)) => {
                let req_err = match e {
                    wait::Waited::TimedOut => ::error::timedout(None),
                    wait::Waited::Inner(e) => e,
                };

                Err(req_err)
            },
            None => Ok(Async::Ready(None)),
        }
    }
}

impl<T: Into<async_impl::body::Body>> From<http::Response<T>> for Response {
    fn from(r: http::Response<T>) -> Response {
        let response = async_impl::Response::from(r);
        Response::new(response, None, KeepCoreThreadAlive::empty())
    }
}
