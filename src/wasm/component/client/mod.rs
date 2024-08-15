#![allow(warnings)]

use http::header::{Entry, CONTENT_LENGTH, USER_AGENT};
use http::{HeaderMap, HeaderValue, Method};
use std::any::Any;
use std::convert::TryInto;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use std::{fmt, future::Future, sync::Arc};

use crate::wasm::component::{Request, RequestBuilder, Response};
use crate::{Body, IntoUrl};
use wasi::http::outgoing_handler::OutgoingRequest;
use wasi::http::types::{FutureIncomingResponse, OutgoingBody, OutputStream, Pollable};

mod future;
use future::ResponseFuture;

/// A client for making HTTP requests.
#[derive(Default, Debug, Clone)]
pub struct Client {
    config: Arc<Config>,
}

/// A builder to configure a [`Client`].
#[derive(Default, Debug)]
pub struct ClientBuilder {
    config: Config,
}

impl Client {
    /// Constructs a new [`Client`].
    pub fn new() -> Self {
        Client::builder().build().expect("Client::new()")
    }

    /// Constructs a new [`ClientBuilder`].
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
    pub fn execute(&self, request: Request) -> crate::Result<ResponseFuture> {
        self.execute_request(request)
    }

    /// Merge [`Request`] headers with default headers set in [`Config`]
    fn merge_default_headers(&self, req: &mut Request) {
        let headers: &mut HeaderMap = req.headers_mut();
        // Insert without overwriting existing headers
        for (key, value) in self.config.headers.iter() {
            if let Entry::Vacant(entry) = headers.entry(key) {
                entry.insert(value.clone());
            }
        }
    }

    pub(super) fn execute_request(&self, mut req: Request) -> crate::Result<ResponseFuture> {
        self.merge_default_headers(&mut req);
        fetch(req)
    }
}

fn fetch(req: Request) -> crate::Result<ResponseFuture> {
    let headers = wasi::http::types::Fields::new();
    for (name, value) in req.headers() {
        headers
            .append(&name.to_string(), &value.as_bytes().to_vec())
            .map_err(crate::error::builder)?;
    }

    if let Some(body) = req.body().and_then(|b| b.as_bytes()) {
        headers
            .append(
                &CONTENT_LENGTH.to_string(),
                &format!("{}", body.len()).as_bytes().to_vec(),
            )
            .map_err(crate::error::builder)?;
    }

    // Construct `OutgoingRequest`
    let outgoing_request = wasi::http::types::OutgoingRequest::new(headers);
    let url = req.url();

    if url.has_authority() {
        outgoing_request
            .set_authority(Some(url.authority()))
            .map_err(|_| crate::error::request("failed to set authority on request"))?;
    }

    outgoing_request
        .set_path_with_query(Some(url.path()))
        .map_err(|_| crate::error::request("failed to set path with query on request"))?;

    match url.scheme() {
        "http" => outgoing_request.set_scheme(Some(&wasi::http::types::Scheme::Http)),
        "https" => outgoing_request.set_scheme(Some(&wasi::http::types::Scheme::Https)),
        scheme => {
            outgoing_request.set_scheme(Some(&wasi::http::types::Scheme::Other(scheme.to_string())))
        }
    }
    .map_err(|_| crate::error::request("failed to set scheme on request"))?;

    match req.method() {
        &Method::GET => outgoing_request.set_method(&wasi::http::types::Method::Get),
        &Method::POST => outgoing_request.set_method(&wasi::http::types::Method::Post),
        &Method::PUT => outgoing_request.set_method(&wasi::http::types::Method::Put),
        &Method::DELETE => outgoing_request.set_method(&wasi::http::types::Method::Delete),
        &Method::HEAD => outgoing_request.set_method(&wasi::http::types::Method::Head),
        &Method::OPTIONS => outgoing_request.set_method(&wasi::http::types::Method::Options),
        &Method::CONNECT => outgoing_request.set_method(&wasi::http::types::Method::Connect),
        &Method::PATCH => outgoing_request.set_method(&wasi::http::types::Method::Patch),
        &Method::TRACE => outgoing_request.set_method(&wasi::http::types::Method::Trace),
        // The only other methods are ExtensionInline and ExtensionAllocated, which are
        // private first of all (can't match on it here) and don't have a strongly typed
        // version in wasi-http, so we fall back to Other.
        _ => {
            outgoing_request.set_method(&wasi::http::types::Method::Other(req.method().to_string()))
        }
    }
    .map_err(|_| {
        crate::error::builder(format!(
            "failed to set method, invalid method {}",
            req.method().to_string()
        ))
    })?;

    ResponseFuture::new(req, outgoing_request)
}

impl ClientBuilder {
    /// Return a new `ClientBuilder`.
    pub fn new() -> Self {
        ClientBuilder {
            config: Config::default(),
        }
    }

    /// Returns a 'Client' that uses this ClientBuilder configuration
    pub fn build(mut self) -> Result<Client, crate::Error> {
        if let Some(err) = self.config.error {
            return Err(err);
        }

        let config = std::mem::take(&mut self.config);
        Ok(Client {
            config: Arc::new(config),
        })
    }

    /// Sets the `User-Agent` header to be used by this client.
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
        }
        self
    }

    /// Sets the default headers for every request
    pub fn default_headers(mut self, headers: HeaderMap) -> ClientBuilder {
        for (key, value) in headers.iter() {
            self.config.headers.insert(key, value.clone());
        }
        self
    }
}

#[derive(Default, Debug)]
struct Config {
    headers: HeaderMap,
    error: Option<crate::Error>,
}

impl Config {
    fn fmt_fields(&self, f: &mut fmt::DebugStruct<'_, '_>) {
        f.field("default_headers", &self.headers);
    }
}
