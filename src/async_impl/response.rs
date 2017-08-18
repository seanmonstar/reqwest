use std::fmt;
use std::marker::PhantomData;

use futures::{Async, Future, Poll, Stream};
use futures::stream::Concat2;
use hyper::StatusCode;
use serde::de::DeserializeOwned;
use serde_json;
use url::Url;

use super::{body, Body};

use std::io::{self, Read};
use libflate::gzip::Decoder;
use header::{Headers, ContentEncoding, ContentLength, Encoding, TransferEncoding};

/// A Response to a submitted `Request`.
pub struct Response {
    status: StatusCode,
    headers: Headers,
    url: Url,
    body: Body,
}

impl Response {
    /// Get the final `Url` of this `Response`.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Get the `StatusCode` of this `Response`.
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the `Headers` of this `Response`.
    #[inline]
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// Get a mutable reference to the `Headers` of this `Response`.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut Headers {
        &mut self.headers
    }

    /// Get a mutable reference to the `Body` of this `Response`.
    #[inline]
    pub fn body_mut(&mut self) -> &mut Body {
        &mut self.body
    }

    /// Resolve to response body.
    #[inline]
    pub fn body_resolved(&mut self) -> BodyFuture {
        let content_encoding_gzip: bool;
        let mut is_gzip = {
            content_encoding_gzip = self.headers()
                .get::<ContentEncoding>()
                .map_or(false, |encs| encs.contains(&Encoding::Gzip));
            content_encoding_gzip ||
            self.headers()
                .get::<TransferEncoding>()
                .map_or(false, |encs| encs.contains(&Encoding::Gzip))
        };
        if is_gzip {
            if let Some(content_length) = self.headers().get::<ContentLength>() {
                if content_length.0 == 0 {
                    warn!("GZipped response with content-length of 0");
                    is_gzip = false;
                }
            }
        }

        trace!("is_gzip: {}", is_gzip);

        BodyFuture {
            concat: body::take(self.body_mut()).concat2(),
            is_gzip: is_gzip,
        }
    }

    /// Try to deserialize the response body as JSON using `serde`.
    #[inline]
    pub fn json<T: DeserializeOwned>(&mut self) -> Json<T> {
        Json {
            concat: body::take(self.body_mut()).concat2(),
            _marker: PhantomData,
        }
    }

    /// Turn a response into an error if the server returned an error.
    // XXX: example disabled since rustdoc still tries to run it
    // when the 'unstable' feature isn't active, making the import
    // fail.
    //
    // # Example
    //
    // ```
    // # use reqwest::unstable::async::Response;
    // fn on_response(res: Response) {
    //     match res.error_for_status() {
    //         Ok(_res) => (),
    //         Err(err) => {
    //             // asserting a 400 as an example
    //             // it could be any status between 400...599
    //             assert_eq!(
    //                 err.status(),
    //                 Some(reqwest::StatusCode::BadRequest)
    //             );
    //         }
    //     }
    // }
    // ```
    #[inline]
    pub fn error_for_status(self) -> ::Result<Self> {
        if self.status.is_client_error() {
            Err(::error::client_error(self.url, self.status))
        } else if self.status.is_server_error() {
            Err(::error::server_error(self.url, self.status))
        } else {
            Ok(self)
        }
    }
}


impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Response")
            .field("url", self.url())
            .field("status", &self.status())
            .field("headers", self.headers())
            .finish()
    }
}

pub struct BodyFuture {
    concat: Concat2<Body>,
    is_gzip: bool,
}

impl Future for BodyFuture {
    type Item = Vec<u8>;
    type Error = ::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let bytes = try_ready!(self.concat.poll());

        if !self.is_gzip {
            return Ok(Async::Ready(bytes.to_vec()))
        }
        let mut buffer = Vec::new();

        let mut decoder = try_!(Decoder::new(&*bytes));
        try_!(io::copy(&mut decoder, &mut buffer));

        Ok(Async::Ready(buffer))
    }
}

impl fmt::Debug for BodyFuture {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BodyFuture")
            .finish()
    }
}


pub struct Json<T> {
    concat: Concat2<Body>,
    _marker: PhantomData<T>,
}

impl<T: DeserializeOwned> Future for Json<T> {
    type Item = T;
    type Error = ::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let bytes = try_ready!(self.concat.poll());
        let t = try_!(serde_json::from_slice(&bytes));
        Ok(Async::Ready(t))
    }
}

impl<T> fmt::Debug for Json<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Json")
            .finish()
    }
}

// pub(crate)

pub fn new(mut res: ::hyper::client::Response, url: Url, _gzip: bool) -> Response {
    use std::mem;

    let status = res.status();
    let headers = mem::replace(res.headers_mut(), Headers::new());
    let body = res.body();
    debug!("Response: '{}' for {}", status, url);
    Response {
        status: status,
        headers: headers,
        url: url,
        body: super::body::wrap(body),
    }
}
