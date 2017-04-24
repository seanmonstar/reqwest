use std::fmt;
use std::io::{self, Read};

use hyper::header::{Headers, ContentEncoding, ContentLength, Encoding, TransferEncoding};
use hyper::status::StatusCode;
use hyper::version::HttpVersion;
use hyper::Url;
use serde::de::DeserializeOwned;
use serde_json;


/// A Response to a submitted `Request`.
pub struct Response {
    inner: Decoder,
}

pub fn new(res: ::hyper::client::Response, gzip: bool) -> Response {
    Response {
        inner: Decoder::from_hyper_response(res, gzip)
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        return match &self.inner {
            &Decoder::PlainText(ref hyper_response) => {
                f.debug_struct("Response")
                    .field("url", &hyper_response.url)
                    .field("status", &hyper_response.status)
                    .field("headers", &hyper_response.headers)
                    .field("version", &hyper_response.version)
                    .finish()
            },
            &Decoder::Gzip{ref url, ref status, ref version, ref headers, ..} => {
                f.debug_struct("Response")
                    .field("url", &url)
                    .field("status", &status)
                    .field("headers", &headers)
                    .field("version", &version)
                    .finish()
            }
        }
    }
}

impl Response {
    /// Get the final `Url` of this response.
    #[inline]
    pub fn url(&self) -> &Url {
        match &self.inner {
            &Decoder::PlainText(ref hyper_response) => &hyper_response.url,
            &Decoder::Gzip{ref url, ..} => url,
        }
    }

    /// Get the `StatusCode`.
    #[inline]
    pub fn status(&self) -> &StatusCode {
        match &self.inner {
            &Decoder::PlainText(ref hyper_response) => &hyper_response.status,
            &Decoder::Gzip{ref status, ..} => status
        }
    }

    /// Get the `Headers`.
    #[inline]
    pub fn headers(&self) -> &Headers {
        match &self.inner {
            &Decoder::PlainText(ref hyper_response) => &hyper_response.headers,
            &Decoder::Gzip{ref headers, ..} => headers
        }
    }

    /// Get the `HttpVersion`.
    #[inline]
    pub fn version(&self) -> &HttpVersion {
        match &self.inner {
            &Decoder::PlainText(ref hyper_response) => &hyper_response.version,
            &Decoder::Gzip{ref version, ..} => version
        }
    }

    /// Try and deserialize the response body as JSON.
    #[inline]
    pub fn json<T: DeserializeOwned>(&mut self) -> ::Result<T> {
        serde_json::from_reader(self).map_err(::Error::from)
    }
}

enum Decoder {
    /// A `PlainText` decoder just returns the response content as is.
    PlainText(::hyper::client::Response),
    /// A `Gzip` decoder will uncompress the gziped response content before returning it.
    Gzip {
        decoder: ::libflate::gzip::Decoder<::hyper::client::Response>,
        url: ::hyper::Url,
        headers: ::hyper::header::Headers,
        version: ::hyper::version::HttpVersion,
        status: ::hyper::status::StatusCode,
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
            content_encoding_gzip = res.headers.get::<ContentEncoding>().map_or(false, |encs|{
                encs.contains(&Encoding::Gzip)
            });
            content_encoding_gzip || res.headers.get::<TransferEncoding>().map_or(false, |encs|{
                encs.contains(&Encoding::Gzip)
            })
        };
        if content_encoding_gzip {
            res.headers.remove::<ContentEncoding>();
            res.headers.remove::<ContentLength>();
        }
        if is_gzip {
            if let Some(content_length) = res.headers.get::<ContentLength>() {
                if content_length.0 == 0 {
                    warn!("GZipped response with content-length of 0");
                    is_gzip = false;
                }
            }
        }
        if is_gzip {
            return Decoder::Gzip {
                url: res.url.clone(),
                status: res.status.clone(),
                version: res.version.clone(),
                headers: res.headers.clone(),
                decoder: ::libflate::gzip::Decoder::new(res).unwrap(),
            };
        } else {
            return Decoder::PlainText(res);
        }
    }
}

impl Read for Decoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            &mut Decoder::PlainText(ref mut hyper_response) => {
                hyper_response.read(buf)
            },
            &mut Decoder::Gzip{ref mut decoder, ..} => {
                decoder.read(buf)
            }
        }
    }
}

/// Read the body of the Response.
impl Read for Response {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

