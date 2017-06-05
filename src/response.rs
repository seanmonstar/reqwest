use std::fmt;
use std::io::{self, Read};

use hyper::header::{Headers, ContentEncoding, ContentLength, Encoding, TransferEncoding};
use hyper::status::StatusCode;
use hyper::Url;
use libflate::gzip;
use serde::de::DeserializeOwned;
use serde_json;


/// A Response to a submitted `Request`.
pub struct Response {
    inner: Decoder,
}

pub fn new(res: ::hyper::client::Response, gzip: bool) -> Response {
    info!("Response: '{}' for {}", res.status, res.url);
    Response {
        inner: Decoder::from_hyper_response(res, gzip),
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.inner {
            Decoder::PlainText(ref hyper_response) => {
                f.debug_struct("Response")
                    .field("url", &hyper_response.url)
                    .field("status", &hyper_response.status)
                    .field("headers", &hyper_response.headers)
                    .finish()
            }
            Decoder::Gzip { ref head, .. } |
            Decoder::Errored { ref head, .. } => {
                f.debug_struct("Response")
                    .field("url", &head.url)
                    .field("status", &head.status)
                    .field("headers", &head.headers)
                    .finish()
            }
        }
    }
}

impl Response {
    /// Get the final `Url` of this response.
    #[inline]
    pub fn url(&self) -> &Url {
        match self.inner {
            Decoder::PlainText(ref hyper_response) => &hyper_response.url,
            Decoder::Gzip { ref head, .. } |
            Decoder::Errored { ref head, .. } => &head.url,
        }
    }

    /// Get the `StatusCode`.
    #[inline]
    pub fn status(&self) -> &StatusCode {
        match self.inner {
            Decoder::PlainText(ref hyper_response) => &hyper_response.status,
            Decoder::Gzip { ref head, .. } |
            Decoder::Errored { ref head, .. } => &head.status,
        }
    }

    /// Get the `Headers`.
    #[inline]
    pub fn headers(&self) -> &Headers {
        match self.inner {
            Decoder::PlainText(ref hyper_response) => &hyper_response.headers,
            Decoder::Gzip { ref head, .. } |
            Decoder::Errored { ref head, .. } => &head.headers,
        }
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
    /// struct Response {
    ///     origin: String,
    /// }
    ///
    /// # fn run() -> Result<(), Error> {
    /// let resp: Response = reqwest::get("http://127.0.0.1/user.json")?.json()?;
    /// # Ok(())
    /// # }
    /// #
    /// # fn main() {
    /// #     if let Err(error) = run() {
    /// #         println!("Error: {:?}", error);
    /// #     }
    /// # }
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
        serde_json::from_reader(self).map_err(::error::from)
    }
}

enum Decoder {
    /// A `PlainText` decoder just returns the response content as is.
    PlainText(::hyper::client::Response),
    /// A `Gzip` decoder will uncompress the gziped response content before returning it.
    Gzip {
        decoder: gzip::Decoder<Peeked>,
        head: Head,
    },
    /// An error occured reading the Gzip header, so return that error
    /// when the user tries to read on the `Response`.
    Errored {
        err: Option<io::Error>,
        head: Head,
    }
}

impl Decoder {
    /// Constructs a Decoder from a hyper request.
    ///
    /// A decoder is just a wrapper around the hyper request that knows
    /// how to decode the content body of the request.
    ///
    /// Uses the correct variant by inspecting the Content-Encoding header.
    fn from_hyper_response(mut res: ::hyper::client::Response, check_gzip: bool) -> Self {
        if !check_gzip {
            return Decoder::PlainText(res);
        }
        let content_encoding_gzip: bool;
        let mut is_gzip = {
            content_encoding_gzip = res.headers
                .get::<ContentEncoding>()
                .map_or(false, |encs| encs.contains(&Encoding::Gzip));
            content_encoding_gzip ||
            res.headers
                .get::<TransferEncoding>()
                .map_or(false, |encs| encs.contains(&Encoding::Gzip))
        };
        if is_gzip {
            if let Some(content_length) = res.headers.get::<ContentLength>() {
                if content_length.0 == 0 {
                    warn!("GZipped response with content-length of 0");
                    is_gzip = false;
                }
            }
        }
        if content_encoding_gzip {
            res.headers.remove::<ContentEncoding>();
            res.headers.remove::<ContentLength>();
        }
        if is_gzip {
            new_gzip(res)
        } else {
            Decoder::PlainText(res)
        }
    }
}

fn new_gzip(mut res: ::hyper::client::Response) -> Decoder {
    // libflate does a read_exact([0; 2]), so its impossible to tell
    // if the stream was empty, or truly had an UnexpectedEof.
    // Therefore, we need to peek a byte to make check for EOF first.
    let mut peek = [0];
    match res.read(&mut peek) {
        Ok(0) => return Decoder::PlainText(res),
        Ok(n) => {
            debug_assert_eq!(n, 1);
        }
        Err(e) => return Decoder::Errored {
            err: Some(e),
            head: Head {
                headers: res.headers.clone(),
                status: res.status,
                url: res.url.clone(),
            }
        }
    }

    let head = Head {
        headers: res.headers.clone(),
        status: res.status,
        url: res.url.clone(),
    };

    let reader = Peeked {
        peeked: Some(peek[0]),
        inner: res,
    };
    match gzip::Decoder::new(reader) {
        Ok(gzip) => Decoder::Gzip {
            decoder: gzip,
            head: head,
        },
        Err(e) => Decoder::Errored {
            err: Some(e),
            head: head,
        }
    }
}

struct Head {
    headers: ::hyper::header::Headers,
    url: ::hyper::Url,
    status: ::hyper::status::StatusCode,
}

struct Peeked {
    peeked: Option<u8>,
    inner: ::hyper::client::Response,
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
            Decoder::PlainText(ref mut hyper_response) => hyper_response.read(buf),
            Decoder::Gzip { ref mut decoder, .. } => decoder.read(buf),
            Decoder::Errored { ref mut err, .. } => {
                Err(err.take().unwrap_or_else(previously_errored))
            }
        }
    }
}

#[inline]
fn previously_errored() -> io::Error {
    io::Error::new(io::ErrorKind::Other, "permanently errored")
}

/// Read the body of the Response.
impl Read for Response {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}
