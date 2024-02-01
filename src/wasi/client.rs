use http::header::{
    HeaderMap, HeaderValue, ACCEPT, USER_AGENT,
};
use http::{HeaderName, Method, StatusCode};
use std::convert::{TryInto, TryFrom};
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use crate::{Body, IntoUrl};
use crate::error::Kind;
use super::request::{Request, RequestBuilder};
use super::response::Response;

use super::wasi::clocks::*;
use super::wasi::http::*;
use super::wasi::io::*;

#[derive(Debug)]
struct Config {
    headers: HeaderMap,
    connect_timeout: Option<Duration>,
    timeout: Option<Duration>,
    error: Option<crate::Error>,
}

/// A `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value. To configure a
/// `Client`, use `Client::builder()`.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and **reuse** it.
///
/// # Examples
///
/// ```rust
/// use reqwest::blocking::Client;
/// #
/// # fn run() -> Result<(), reqwest::Error> {
/// let client = Client::new();
/// let resp = client.get("http://httpbin.org/").send()?;
/// #   drop(resp);
/// #   Ok(())
/// # }
///
///
#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<ClientRef>,
}

#[derive(Debug)]
struct ClientRef {
    pub headers: HeaderMap,
    pub connect_timeout: Option<Duration>,
    pub first_byte_timeout: Option<Duration>,
    pub between_bytes_timeout: Option<Duration>,
}

/// A `ClientBuilder` can be used to create a `Client` with  custom configuration.
///
/// # Example
///
/// ```
/// # fn run() -> Result<(), reqwest::Error> {
/// use std::time::Duration;
///
/// let client = reqwest::blocking::Client::builder()
///     .timeout(Duration::from_secs(10))
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[must_use]
#[derive(Debug)]
pub struct ClientBuilder {
    config: Config,
}

impl Client {
    /// Constructs a new `Client`.
    pub fn new() -> Self {
        Client::builder().build().expect("Client::new()")
    }

    /// Creates a `ClientBuilder` to configure a `Client`.
    ///
    /// This is the same as `ClientBuilder::new()`.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }


    /// Convenience method to make a `GET` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    /// Convenience method to make a `POST` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    /// Convenience method to make a `PUT` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn put<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PUT, url)
    }

    /// Convenience method to make a `PATCH` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn patch<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PATCH, url)
    }

    /// Convenience method to make a `DELETE` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn delete<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }

    /// Convenience method to make a `HEAD` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn head<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }

    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// request body before sending.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        let req = url.into_url().map(move |url| Request::new(method, url));
        RequestBuilder::new(self.clone(), req)
    }

    /// Executes a `Request`.
    ///
    /// A `Request` can be built manually with `Request::new()` or obtained
    /// from a RequestBuilder with `RequestBuilder::build()`.
    ///
    /// You should prefer to use the `RequestBuilder` and
    /// `RequestBuilder::send()`.
    ///
    /// # Errors
    ///
    /// This method fails if there was an error while sending request,
    /// redirect loop was detected or redirect limit was exhausted.
    pub fn execute(
        &self,
        request: Request,
    ) -> Result<Response, crate::Error> {
        let mut header_key_values: Vec<(String, Vec<u8>)> = vec![];
        for (name, value) in self.inner.headers.iter() {
            match value.to_str() {
                Ok(value) => header_key_values.push((name.as_str().to_string(), value.into())),
                Err(_) => {}
            }
        }

        let (method, url, headers, body, timeout, _version) = request.pieces();
        for (name, value) in headers.iter() {
            match value.to_str() {
                Ok(value) => header_key_values.push((name.as_str().to_string(), value.into())),
                Err(_) => {}
            }
        }

        let scheme = match url.scheme() {
            "http" => types::Scheme::Http,
            "https" => types::Scheme::Https,
            other => types::Scheme::Other(other.to_string())
        };
        let headers = types::Fields::from_list(&header_key_values)?;
        let request = types::OutgoingRequest::new(headers);
        let path_with_query = match url.query() {
            Some(query) => format!("{}?{}", url.path(), query),
            None => url.path().to_string()
        };
        request.set_method(&method.into()).map_err(|e| failure_point("set_method", e))?;
        request.set_path_with_query(Some(&path_with_query)).map_err(|e| failure_point("set_path_with_query", e))?;
        request.set_scheme(Some(&scheme)).map_err(|e| failure_point("set_scheme", e))?;
        request.set_authority(Some(url.authority())).map_err(|e| failure_point("set_authority", e))?;

        match body {
            Some(body) => {
                let request_body = request.body().map_err(|e| failure_point("body", e))?;
                let request_body_stream = request_body.write().map_err(|e| failure_point("write", e))?;
                body.write(|chunk| {
                    request_body_stream.write(chunk)?;
                    Ok(())
                })?;
                drop(request_body_stream);
                types::OutgoingBody::finish(request_body, None)?;
            }
            None => {}
        }

        let options = types::RequestOptions::new();
        options.set_connect_timeout(self.inner.connect_timeout.map(|d| d.as_millis() as u64)).map_err(|e| failure_point("set_connect_timeout_ms", e))?;
        options.set_first_byte_timeout(timeout.or(self.inner.first_byte_timeout).map(|d| d.as_millis() as u64)).map_err(|e| failure_point("set_first_byte_timeout_ms", e))?;
        options.set_between_bytes_timeout(timeout.or(self.inner.between_bytes_timeout).map(|d| d.as_millis() as u64)).map_err(|e| failure_point("set_between_bytes_timeout_ms", e))?;

        let future_incoming_response = outgoing_handler::handle(request, Some(options))?;

        let receive_timeout = timeout.or(self.inner.first_byte_timeout);
        let incoming_response = Self::get_incoming_response(&future_incoming_response, receive_timeout)?;

        let status = incoming_response.status();
        let status_code = StatusCode::from_u16(status).map_err(|e| crate::Error::new(crate::error::Kind::Decode, Some(e)))?;

        let response_fields = incoming_response.headers();
        let response_headers = Self::fields_to_header_map(&response_fields);

        let response_body = incoming_response.consume().map_err(|e| failure_point("consume", e))?;
        let response_body_stream = response_body.stream().map_err(|e| failure_point("stream", e))?;
        let body: Body = response_body_stream.into();

        Ok(Response::new(status_code, response_headers, body, incoming_response, response_body, url))
    }

    fn get_incoming_response(future_incoming_response: &types::FutureIncomingResponse, timeout: Option<Duration>) -> Result<types::IncomingResponse, crate::Error> {
        let deadline_pollable = monotonic_clock::subscribe_duration(timeout.unwrap_or(Duration::from_secs(10000000000)).as_nanos() as u64);
        loop {
            match future_incoming_response.get() {
                Some(Ok(Ok(incoming_response))) => {
                    return Ok(incoming_response);
                }
                Some(Ok(Err(err))) => return Err(err.into()),
                Some(Err(err)) => return Err(failure_point("get_incoming_response", err)),
                None => {
                    let pollable = future_incoming_response.subscribe();
                    let bitmap = poll::poll(&[&pollable, &deadline_pollable]);
                    if timeout.is_none() || !bitmap.contains(&1) {
                        continue;
                    } else {
                        return Err(crate::Error::new(Kind::Request, Some(crate::error::TimedOut)));
                    }
                }
            };
        }
    }

    fn fields_to_header_map(fields: &types::Fields) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let entries = fields.entries();
        for (name, value) in entries {
            headers.insert(HeaderName::try_from(&name).expect("Invalid header name"),
                           HeaderValue::from_bytes(&value).expect("Invalid header value"));
        }
        headers
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientBuilder {
    /// Constructs a new `ClientBuilder`.
    ///
    /// This is the same as `Client::builder()`.
    pub fn new() -> Self {
        let mut headers: HeaderMap<HeaderValue> = HeaderMap::with_capacity(2);
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));

        Self {
            config: Config {
                headers,
                connect_timeout: None,
                timeout: None,
                error: None,
            }
        }
    }

    /// Returns a `Client` that uses this `ClientBuilder` configuration.
    ///
    /// # Errors
    ///
    /// This method fails if TLS backend cannot be initialized, or the resolver
    /// cannot load the system configuration.
    pub fn build(self) -> Result<Client, crate::Error> {
        if let Some(err) = self.config.error {
            return Err(err);
        }

        Ok(Client {
            inner: Arc::new(
                ClientRef {
                    headers: self.config.headers,
                    connect_timeout: self.config.connect_timeout,
                    first_byte_timeout: self.config.timeout,
                    between_bytes_timeout: self.config.timeout,
                }
            )
        })
    }

    /// Sets the `User-Agent` header to be used by this client.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn doc() -> Result<(), reqwest::Error> {
    /// // Name your user agent after your app?
    /// static APP_USER_AGENT: &str = concat!(
    ///     env!("CARGO_PKG_NAME"),
    ///     "/",
    ///     env!("CARGO_PKG_VERSION"),
    /// );
    ///
    /// let client = reqwest::blocking::Client::builder()
    ///     .user_agent(APP_USER_AGENT)
    ///     .build()?;
    /// let res = client.get("https://www.rust-lang.org").send()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn user_agent<V>(mut self, value: V) -> ClientBuilder
        where
            V: TryInto<HeaderValue>,
            V::Error: Into<http::Error>,
    {
        match value.try_into() {
            Ok(value) => {
                self.config.headers.insert(USER_AGENT, value);
            }
            Err(e) => {
                self.config.error = Some(crate::error::builder(e.into()));
            }
        };
        self
    }

    /// Sets the default headers for every request
    pub fn default_headers(mut self, headers: HeaderMap) -> ClientBuilder {
        for (key, value) in headers.iter() {
            self.config.headers.insert(key, value.clone());
        }
        self
    }

    // TODO: cookie support
    // TODO: gzip support
    // TODO: brotli support
    // TODO: deflate support
    // TODO: redirect support
    // TODO: proxy support
    // TODO: TLS support

    // Timeout options

    /// Set a timeout for connect, read and write operations of a `Client`.
    ///
    /// Default is 30 seconds.
    ///
    /// Pass `None` to disable timeout.
    pub fn timeout<T>(mut self, timeout: T) -> ClientBuilder
        where
            T: Into<Option<Duration>>,
    {
        self.config.timeout = timeout.into();
        self
    }

    /// Set a timeout for only the connect phase of a `Client`.
    ///
    /// Default is `None`.
    pub fn connect_timeout<T>(mut self, timeout: T) -> ClientBuilder
        where
            T: Into<Option<Duration>>,
    {
        self.config.connect_timeout = timeout.into();
        self
    }
}

impl From<Method> for types::Method {
    fn from(value: Method) -> types::Method {
        if value == Method::GET {
            types::Method::Get
        } else if value == Method::POST {
            types::Method::Post
        } else if value == Method::PUT {
            types::Method::Put
        } else if value == Method::DELETE {
            types::Method::Delete
        } else if value == Method::HEAD {
            types::Method::Head
        } else if value == Method::OPTIONS {
            types::Method::Options
        } else if value == Method::CONNECT {
            types::Method::Connect
        } else if value == Method::PATCH {
            types::Method::Patch
        } else if value == Method::TRACE {
            types::Method::Trace
        } else {
            types::Method::Other(value.as_str().to_string())
        }
    }
}

impl From<types::ErrorCode> for crate::Error {
    fn from(value: types::ErrorCode) -> Self {
        crate::Error::new(Kind::Request, Some(std::io::Error::new(ErrorKind::Other, format!("{:?}", value))))
    }
}

impl From<streams::StreamError> for crate::Error {
    fn from(value: streams::StreamError) -> Self {
        crate::Error::new(Kind::Request, Some(std::io::Error::new(ErrorKind::Other, format!("{:?}", value))))
    }
}

impl From<types::HeaderError> for crate::Error {
    fn from(value: types::HeaderError) -> Self {
        crate::Error::new(Kind::Request, Some(std::io::Error::new(ErrorKind::Other, format!("{:?}", value))))
    }
}

pub(crate) fn failure_point(s: &str, _: ()) -> crate::Error {
    crate::Error::new(Kind::Request, Some(std::io::Error::new(ErrorKind::Other, s)))
}