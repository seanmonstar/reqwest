use std::fmt;
use std::mem;
use std::marker::PhantomData;

use futures::{Async, Future, Poll, Stream};
use futures::stream::Concat2;
use hyper::{HeaderMap, StatusCode, Version};
use serde::de::DeserializeOwned;
use serde_json;
use url::Url;

use super::{decoder, body, Decoder};


/// A Response to a submitted `Request`.
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    // Boxed to save space (11 words to 1 word), and it's not accessed
    // frequently internally.
    url: Box<Url>,
    body: Decoder,
    version: Version,
}

impl Response {
    pub(super) fn new(mut res: ::hyper::Response<::hyper::Body>, url: Url, gzip: bool) -> Response {
        let status = res.status();
        let version = res.version();
        let mut headers = mem::replace(res.headers_mut(), HeaderMap::new());
        let decoder = decoder::detect(&mut headers, body::wrap(res.into_body()), gzip);
        debug!("Response: '{}' for {}", status, url);
        Response {
            status,
            headers,
            url: Box::new(url),
            body: decoder,
            version,
        }
    }

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
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to the `Headers` of this `Response`.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
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

    /// Get the HTTP `Version` of this `Response`.
    #[inline]
    pub fn version(&self) -> Version {
        self.version
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
    /// # use reqwest::async::Response;
    /// fn on_response(res: Response) {
    ///     match res.error_for_status() {
    ///         Ok(_res) => (),
    ///         Err(err) => {
    ///             // asserting a 400 as an example
    ///             // it could be any status between 400...599
    ///             assert_eq!(
    ///                 err.status(),
    ///                 Some(reqwest::StatusCode::BAD_REQUEST)
    ///             );
    ///         }
    ///     }
    /// }
    /// # fn main() {}
    /// ```
    #[inline]
    pub fn error_for_status(self) -> ::Result<Self> {
        if self.status.is_client_error() {
            Err(::error::client_error(*self.url, self.status))
        } else if self.status.is_server_error() {
            Err(::error::server_error(*self.url, self.status))
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

