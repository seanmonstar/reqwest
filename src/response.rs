use std::mem;
use std::fmt;
use std::io::{self, Read};
use std::time::Duration;
use std::borrow::Cow;

use encoding_rs::{Encoding, UTF_8};
use futures::{Async, Poll, Stream};
use mime::Mime;
use serde::de::DeserializeOwned;
use serde_json;

use client::KeepCoreThreadAlive;
use hyper::header::HeaderMap;
use {async_impl, StatusCode, Url, wait};

/// A Response to a submitted `Request`.
pub struct Response {
    inner: async_impl::Response,
    body: async_impl::ReadableChunks<WaitBody>,
    _thread_handle: KeepCoreThreadAlive,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl Response {
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

    /// Get the `StatusCode` of this `Response`.
    ///
    /// # Examples
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
    /// ```rust
    /// use reqwest::Client;
    /// use reqwest::StatusCode;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = Client::new();
    /// let resp = client.post("http://httpbin.org/post")
    ///             .body("possibly too large")
    ///             .send()?;
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
    /// Checking the `Content-Length` header before reading the response body.
    ///
    /// ```rust
    /// # use std::io::{Read, Write};
    /// # use reqwest::Client;
    /// # use reqwest::header::CONTENT_LENGTH;
    /// #
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = Client::new();
    /// let mut resp = client.head("http://httpbin.org/bytes/3000").send()?;
    /// if resp.status().is_success() {
    ///     let len = resp.headers().get(CONTENT_LENGTH)
    ///                 .and_then(|ct_len| ct_len.to_str().ok())
    ///                 .and_then(|ct_len| ct_len.parse().ok())
    ///                 .unwrap_or(0);
    ///     // limit 1mb response
    ///     if len <= 1_000_000 {
    ///         let mut buf = Vec::with_capacity(len as usize);
    ///         let mut resp = reqwest::get("http://httpbin.org/bytes/3000")?;
    ///         if resp.status().is_success() {
    ///             ::std::io::copy(&mut resp, &mut buf)?;
    ///         }
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        self.inner.headers()
    }

    /// Try and deserialize the response body as JSON using `serde`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate reqwest;
    /// # #[macro_use] extern crate serde_derive;
    /// #
    /// # use reqwest::Error;
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
        // There's 2 ways we could implement this:
        //
        // 1. Just using from_reader(self), making use of our blocking read adapter
        // 2. Just use self.inner.json().wait()
        //
        // Doing 1 is pretty easy, but it means we have the `serde_json` code
        // in more than one place, doing basically the same thing.
        //
        // Doing 2 would mean `serde_json` is only in one place, but we'd
        // need to update the sync Response to lazily make a blocking read
        // adapter, so that our `inner` could possibly still have the original
        // body.
        //
        // Went for easier for now, just to get it working.
        serde_json::from_reader(self).map_err(::error::from)
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
        // FIXME: get Body::content_length() instead
        let len = self.headers().get(::header::CONTENT_LENGTH)
            .and_then(|ct_len| ct_len.to_str().ok())
            .and_then(|ct_len| ct_len.parse().ok())
            .unwrap_or(0);
        let mut content = Vec::with_capacity(len as usize);
        self.read_to_end(&mut content).map_err(::error::from)?;
        let content_type = self.headers().get(::header::CONTENT_TYPE)
            .and_then(|value| {
                value.to_str().ok()
            })
            .and_then(|value| {
                value.parse::<Mime>().ok()
            });
        let encoding_name = content_type
            .as_ref()
            .and_then(|mime| {
                mime
                    .get_param("charset")
                    .map(|charset| charset.as_str())
            })
            .unwrap_or("utf-8");
        let encoding = Encoding::for_label(encoding_name.as_bytes()).unwrap_or(UTF_8);
        // a block because of borrow checker
        {
            let (text, _, _) = encoding.decode(&content);
            match text {
                Cow::Owned(s) => return Ok(s),
                _ => (),
            }
        }
        unsafe {
            // decoding returned Cow::Borrowed, meaning these bytes
            // are already valid utf8
            Ok(String::from_utf8_unchecked(content))
        }
    }

    /// Copy the response body into a writer.
    ///
    /// This function internally uses [`std::io::copy`] and hence will continuously read data from
    /// the body and then write it into writer in a streaming fashion until EOF is met.
    ///
    /// On success, the total number of bytes that were copied to `writer` is returned.
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
        let Response { body, inner, _thread_handle } = self;
        inner.error_for_status().map(move |inner| {
            Response {
                inner: inner,
                body: body,
                _thread_handle: _thread_handle,
            }
        })
    }
}

impl Read for Response {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.body.read(buf)
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
                    wait::Waited::Err(e) => e,
                };

                Err(req_err)
            },
            None => Ok(Async::Ready(None)),
        }
    }
}

// pub(crate)

pub fn new(mut res: async_impl::Response, timeout: Option<Duration>, thread: KeepCoreThreadAlive) -> Response {
    let body = mem::replace(res.body_mut(), async_impl::Decoder::empty());
    let body = async_impl::ReadableChunks::new(WaitBody {
        inner: wait::stream(body, timeout)
    });

    Response {
        inner: res,
        body: body,
        _thread_handle: thread,
    }
}
