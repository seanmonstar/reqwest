use std::io::{self, Read};

use hyper::header::{Headers, ContentType, UserAgent};
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::version::HttpVersion;
use hyper::{Url};

use serde::Serialize;
use serde_json;

use ::body::{self, Body};

static DEFAULT_USER_AGENT: &'static str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// A `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and reuse it.
pub struct Client {
    inner: ::hyper::Client,
}

impl Client {
    /// Constructs a new `Client`.
    pub fn new() -> Client {
        Client {
            inner: new_hyper_client()
        }
    }

    /// Convenience method to make a `GET` request to a URL.
    pub fn get(&self, url: &str) -> RequestBuilder {
        self.request(Method::Get, Url::parse(url).unwrap())
    }

    /// Convenience method to make a `POST` request to a URL.
    pub fn post(&self, url: &str) -> RequestBuilder {
        self.request(Method::Post, Url::parse(url).unwrap())
    }

    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// request body before sending.
    pub fn request(&self, method: Method, url: Url) -> RequestBuilder {
        debug!("request {:?} \"{}\"", method, url);
        RequestBuilder {
            client: self,
            method: method,
            url: url,
            _version: HttpVersion::Http11,
            headers: Headers::new(),

            body: None,
        }
    }
}

#[cfg(not(feature = "tls"))]
fn new_hyper_client() -> ::hyper::Client {
    ::hyper::Client::new()
}

#[cfg(feature = "tls")]
fn new_hyper_client() -> ::hyper::Client {
    use tls::TlsClient;
    ::hyper::Client::with_connector(
        ::hyper::client::Pool::with_connector(
            Default::default(),
            ::hyper::net::HttpsConnector::new(TlsClient::new().unwrap())
        )
    )
}


/// A builder to construct the properties of a `Request`.
pub struct RequestBuilder<'a> {
    client: &'a Client,

    method: Method,
    url: Url,
    _version: HttpVersion,
    headers: Headers,

    body: Option<Body>,
}

impl<'a> RequestBuilder<'a> {
    /// Add a `Header` to this Request.
    pub fn header<H: ::header::Header + ::header::HeaderFormat>(mut self, header: H) -> RequestBuilder<'a> {
        self.headers.set(header);
        self
    }
    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    pub fn headers(mut self, headers: ::header::Headers) -> RequestBuilder<'a> {
        self.headers.extend(headers.iter());
        self
    }

    /// Set the request body.
    pub fn body<T: Into<Body>>(mut self, body: T) -> RequestBuilder<'a> {
        self.body = Some(body.into());
        self
    }

    pub fn json<T: Serialize>(mut self, json: T) -> RequestBuilder<'a> {
        let body = serde_json::to_vec(&json).expect("serde to_vec cannot fail");
        self.headers.set(ContentType::json());
        self.body = Some(body.into());
        self
    }

    /// Constructs the Request and sends it the target URL, returning a Response.
    pub fn send(mut self) -> ::Result<Response> {
        if !self.headers.has::<UserAgent>() {
            self.headers.set(UserAgent(DEFAULT_USER_AGENT.to_owned()));
        }

        let mut req = self.client.inner.request(self.method, self.url)
            .headers(self.headers);

        if let Some(ref b) = self.body {
            let body = body::as_hyper_body(b);
            req = req.body(body);
        }

        let res = try!(req.send());
        Ok(Response {
            inner: res
        })
    }
}

/// A Response to a submitted `Request`.
pub struct Response {
    inner: ::hyper::client::Response,
}

impl Response {
    /// Get the `StatusCode`.
    pub fn status(&self) -> &StatusCode {
        &self.inner.status
    }

    /// Get the `Headers`.
    pub fn headers(&self) -> &Headers {
        &self.inner.headers
    }

    /// Get the `HttpVersion`.
    pub fn version(&self) -> &HttpVersion {
        &self.inner.version
    }
}

/// Read the body of the Response.
impl Read for Response {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}
