use std::fmt;
use std::mem;
use std::io::{self, Read};
use std::marker::PhantomData;

use libflate::non_blocking::gzip;
use tokio_io::AsyncRead;
use tokio_io::io as async_io;
use futures::{Async, Future, Poll, Stream};
use futures::stream::Concat2;
use hyper::StatusCode;
use serde::de::DeserializeOwned;
use serde_json;
use url::Url;

use header::{Headers, ContentEncoding, ContentLength, Encoding, TransferEncoding};
use super::{decoder, body, Body, Chunk, Decoder};
use error;


/// A Response to a submitted `Request`.
pub struct Response {
    status: StatusCode,
    headers: Headers,
    url: Url,
    body: Decoder,
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

    /// Consumes the response, returning the body
    pub fn into_body(self) -> Decoder {
        self.body
    }

    /// Get a reference to the response body.
    #[inline]
    pub fn body(&self) -> &Decoder {
        &self.body
    }

    /// Get a mutable reference to the response body.
    ///
    /// The chunks from the body may be decoded, depending on the `gzip`
    /// option on the `ClientBuilder`.
    #[inline]
    pub fn body_mut(&mut self) -> &mut Decoder {
        &mut self.body
    }


    /// Try to deserialize the response body as JSON using `serde`.
    #[inline]
    pub fn json<T: DeserializeOwned>(&mut self) -> Json<T> {
        let body = mem::replace(&mut self.body, Decoder::empty());

        Json {
            concat: body.concat2(),
            _marker: PhantomData,
        }
    }

    /// Turn a response into an error if the server returned an error.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature="unstable")]
    /// # use reqwest::unstable::async::Response;
    /// # #[cfg(feature="unstable")]
    /// fn on_response(res: Response) {
    ///     match res.error_for_status() {
    ///         Ok(_res) => (),
    ///         Err(err) => {
    ///             // asserting a 400 as an example
    ///             // it could be any status between 400...599
    ///             assert_eq!(
    ///                 err.status(),
    ///                 Some(reqwest::StatusCode::BadRequest)
    ///             );
    ///         }
    ///     }
    /// }
    /// # fn main() {}
    /// ```
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

pub struct Json<T> {
    concat: Concat2<Decoder>,
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

pub fn new(mut res: ::hyper::client::Response, url: Url, gzip: bool) -> Response {
    let status = res.status();
    let mut headers = mem::replace(res.headers_mut(), Headers::new());
    let decoder = decoder::detect(&mut headers, body::wrap(res.body()), gzip);
    debug!("Response: '{}' for {}", status, url);
    Response {
        status: status,
        headers: headers,
        url: url,
        body: decoder,
    }
}
