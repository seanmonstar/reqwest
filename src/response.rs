use std::fmt;
use std::io::{self, Read};
use std::time::Duration;

use libflate::gzip;
use serde::de::DeserializeOwned;
use serde_json;

use client::KeepCoreThreadAlive;
use header::{Headers, ContentEncoding, ContentLength, Encoding, TransferEncoding};
use {async_impl, StatusCode, Url, wait};


/// A Response to a submitted `Request`.
pub struct Response {
    body: Decoder,
    inner: async_impl::Response,
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
    /// let client = Client::new()?;
    /// let resp = client.post("http://httpbin.org/post")?
    ///             .body("possibly too large")
    ///             .send()?;
    /// match resp.status() {
    ///     StatusCode::Ok => println!("success!"),
    ///     StatusCode::PayloadTooLarge => {
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
    /// ```rust
    /// # use std::io::{Read, Write};
    /// # use reqwest::Client;
    /// # use reqwest::header::ContentLength;
    /// #
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut resp = client.head("http://httpbin.org/bytes/3000")?.send()?;
    /// if resp.status().is_success() {
    ///     let len = resp.headers().get::<ContentLength>()
    ///                 .map(|ct_len| **ct_len)
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
    pub fn headers(&self) -> &Headers {
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
    ///     assert_eq!(err.status(), Some(reqwest::StatusCode::BadRequest));
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
                body: body,
                inner: inner,
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

struct ReadableBody {
    state: ReadState,
    stream:  wait::WaitStream<async_impl::Body>,
}

enum ReadState {
    Ready(async_impl::Chunk, usize),
    NotReady,
    Eof,
}


impl Read for ReadableBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use std::cmp;

        loop {
            let ret;
            match self.state {
                ReadState::Ready(ref mut chunk, ref mut pos) => {
                    let chunk_start = *pos;
                    let len = cmp::min(buf.len(), chunk.len() - chunk_start);
                    let chunk_end = chunk_start + len;
                    buf[..len].copy_from_slice(&chunk[chunk_start..chunk_end]);
                    *pos += len;
                    if *pos == chunk.len() {
                        ret = len;
                    } else {
                        return Ok(len);
                    }
                },
                ReadState::NotReady => {
                    match self.stream.next() {
                        Some(Ok(chunk)) => {
                            self.state = ReadState::Ready(chunk, 0);
                            continue;
                        },
                        Some(Err(e)) => {
                            let req_err = match e {
                                wait::Waited::TimedOut => ::error::timedout(None),
                                wait::Waited::Err(e) => e,
                            };
                            return Err(::error::into_io(req_err));
                        },
                        None => {
                            self.state = ReadState::Eof;
                            return Ok(0);
                        },
                    }
                },
                ReadState::Eof => return Ok(0),
            }
            self.state = ReadState::NotReady;
            return Ok(ret);
        }
    }
}


enum Decoder {
    /// A `PlainText` decoder just returns the response content as is.
    PlainText(ReadableBody),
    /// A `Gzip` decoder will uncompress the gziped response content before returning it.
    Gzip(gzip::Decoder<Peeked>),
    /// An error occured reading the Gzip header, so return that error
    /// when the user tries to read on the `Response`.
    Errored(Option<io::Error>),
}

impl Decoder {
    /// Constructs a Decoder from a hyper request.
    ///
    /// A decoder is just a wrapper around the hyper request that knows
    /// how to decode the content body of the request.
    ///
    /// Uses the correct variant by inspecting the Content-Encoding header.
    fn new(res: &mut async_impl::Response, check_gzip: bool, timeout: Option<Duration>) -> Self {
        let body = async_impl::body::take(res.body_mut());
        let body = ReadableBody {
            state: ReadState::NotReady,
            stream: wait::stream(body, timeout),
        };

        if !check_gzip {
            return Decoder::PlainText(body);
        }
        let content_encoding_gzip: bool;
        let mut is_gzip = {
            content_encoding_gzip = res.headers()
                .get::<ContentEncoding>()
                .map_or(false, |encs| encs.contains(&Encoding::Gzip));
            content_encoding_gzip ||
            res.headers()
                .get::<TransferEncoding>()
                .map_or(false, |encs| encs.contains(&Encoding::Gzip))
        };
        if is_gzip {
            if let Some(content_length) = res.headers().get::<ContentLength>() {
                if content_length.0 == 0 {
                    warn!("GZipped response with content-length of 0");
                    is_gzip = false;
                }
            }
        }
        if content_encoding_gzip {
            res.headers_mut().remove::<ContentEncoding>();
            res.headers_mut().remove::<ContentLength>();
        }
        if is_gzip {
            new_gzip(body)
        } else {
            Decoder::PlainText(body)
        }
    }
}

fn new_gzip(mut body: ReadableBody) -> Decoder {
    // libflate does a read_exact([0; 2]), so its impossible to tell
    // if the stream was empty, or truly had an UnexpectedEof.
    // Therefore, we need to peek a byte to make check for EOF first.
    let mut peek = [0];
    match body.read(&mut peek) {
        Ok(0) => return Decoder::PlainText(body),
        Ok(n) => debug_assert_eq!(n, 1),
        Err(e) => return Decoder::Errored(Some(e)),
    }

    let reader = Peeked {
        peeked: Some(peek[0]),
        inner: body,
    };
    match gzip::Decoder::new(reader) {
        Ok(gzip) => Decoder::Gzip(gzip),
        Err(e) => Decoder::Errored(Some(e)),
    }
}

struct Peeked {
    peeked: Option<u8>,
    inner: ReadableBody,
}

impl Read for Peeked {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(byte) = self.peeked.take() {
            buf[0] = byte;
            Ok(1)
        } else {
            self.inner.read(buf)
        }
    }
}

impl Read for Decoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            Decoder::PlainText(ref mut body) => body.read(buf),
            Decoder::Gzip(ref mut decoder) => decoder.read(buf),
            Decoder::Errored(ref mut err) => {
                Err(err.take().unwrap_or_else(previously_errored))
            }
        }
    }
}

#[inline]
fn previously_errored() -> io::Error {
    io::Error::new(io::ErrorKind::Other, "permanently errored")
}


// pub(crate)

pub fn new(mut res: async_impl::Response, gzip: bool, timeout: Option<Duration>, thread: KeepCoreThreadAlive) -> Response {

    let decoder = Decoder::new(&mut res, gzip, timeout);
    Response {
        body: decoder,
        inner: res,
        _thread_handle: thread,
    }
}
