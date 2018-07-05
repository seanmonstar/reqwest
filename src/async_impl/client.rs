use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::{Async, Future, Poll};
use hyper::client::ResponseFuture;
use header::{HeaderMap, HeaderValue, LOCATION, USER_AGENT, REFERER, ACCEPT,
             ACCEPT_ENCODING, RANGE};
use mime::{self};
use native_tls::{TlsConnector, TlsConnectorBuilder};


use super::body;
use super::request::{self, Request, RequestBuilder};
use super::response::{self, Response};
use connect::Connector;
use into_url::to_uri;
use redirect::{self, RedirectPolicy, check_redirect, remove_sensitive_headers};
use {Certificate, Identity, IntoUrl, Method, proxy, Proxy, StatusCode, Url};

static DEFAULT_USER_AGENT: &'static str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// An asynchronous `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and **reuse** it.
///
/// # Examples
///
/// ```rust
/// # #[cfg(features = "unstable")]
/// # fn run() -> Result<(), Box<::std::error::Error>> {
/// # extern crate tokio_core;
/// # extern crate futures;
/// use futures::Future;
/// use tokio_core::reactor::Core;
/// use reqwest::unstable::async::Client;
///
/// let mut core = Core::new()?;
/// let client = Client::new(&core.handle());
/// let fut = client.get("http://httpbin.org/").send();
/// core.run(fut)?;
/// #   Ok(())
/// # }
/// # fn main() {}
/// ```

#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientRef>,
}

/// A `ClientBuilder` can be used to create a `Client` with  custom configuration:
pub struct ClientBuilder {
    config: Option<Config>,
    err: Option<::Error>,
}

struct Config {
    gzip: bool,
    headers: HeaderMap,
    hostname_verification: bool,
    proxies: Vec<Proxy>,
    redirect_policy: RedirectPolicy,
    referer: bool,
    timeout: Option<Duration>,
    tls: TlsConnectorBuilder,
    dns_threads: usize,
}

impl ClientBuilder {
    /// Constructs a new `ClientBuilder`
    pub fn new() -> ClientBuilder {
        match TlsConnector::builder() {
            Ok(tls_connector_builder) => {
                let mut headers: HeaderMap<HeaderValue> = HeaderMap::with_capacity(2);
                headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
                headers.insert(ACCEPT, HeaderValue::from_str(mime::STAR_STAR.as_ref()).expect("unable to parse mime"));

                ClientBuilder {
                    config: Some(Config {
                        gzip: true,
                        headers: headers,
                        hostname_verification: true,
                        proxies: Vec::new(),
                        redirect_policy: RedirectPolicy::default(),
                        referer: true,
                        timeout: None,
                        tls: tls_connector_builder,
                        dns_threads: 4,
                    }),
                    err: None,
                }
            },
            Err(e) => ClientBuilder {
                config: None,
                err: Some(::error::from(e)),
            }
        }
    }

    /// Returns a `Client` that uses this `ClientBuilder` configuration.
    ///
    /// # Errors
    ///
    /// This method fails if native TLS backend cannot be initialized.
    ///
    /// # Panics
    ///
    /// This method consumes the internal state of the builder.
    /// Trying to use this builder again after calling `build` will panic.
    pub fn build(&mut self) -> ::Result<Client> {
        if let Some(err) = self.err.take() {
            return Err(err);
        }
        let config = self.config
            .take()
            .expect("ClientBuilder cannot be reused after building a Client");

        let tls = try_!(config.tls.build());

        let proxies = Arc::new(config.proxies);

        let mut connector = Connector::new(config.dns_threads, tls, proxies.clone());
        if !config.hostname_verification {
            connector.danger_disable_hostname_verification();
        }

        let hyper_client = ::hyper::Client::builder()
            .build(connector);

        Ok(Client {
            inner: Arc::new(ClientRef {
                gzip: config.gzip,
                hyper: hyper_client,
                headers: config.headers,
                proxies: proxies,
                redirect_policy: config.redirect_policy,
                referer: config.referer,
            }),
        })
    }

    /// Add a custom root certificate.
    ///
    /// This can be used to connect to a server that has a self-signed
    /// certificate for example.
    pub fn add_root_certificate(&mut self, cert: Certificate) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            let cert = ::tls::cert(cert);
            if let Err(e) = config.tls.add_root_certificate(cert) {
                self.err = Some(::error::from(e));
            }
        }
        self
    }

    /// Sets the identity to be used for client certificate authentication.
    pub fn identity(&mut self, identity: Identity) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            let pkcs12 = ::tls::pkcs12(identity);
            if let Err(e) = config.tls.identity(pkcs12) {
                self.err = Some(::error::from(e));
            }
        }
        self
    }

    /// Disable hostname verification.
    ///
    /// # Warning
    ///
    /// You should think very carefully before you use this method. If
    /// hostname verification is not used, any valid certificate for any
    /// site will be trusted for use from any other. This introduces a
    /// significant vulnerability to man-in-the-middle attacks.
    #[inline]
    pub fn danger_disable_hostname_verification(&mut self) -> &mut ClientBuilder {

        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.hostname_verification = false;
        }
        self
    }

    /// Enable hostname verification.
    #[inline]
    pub fn enable_hostname_verification(&mut self) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.hostname_verification = true;
        }
        self
    }

    /// Sets the default headers for every request.
    #[inline]
    pub fn default_headers(&mut self, headers: HeaderMap) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            for (key, value) in headers.iter() {
                config.headers.insert(key, value.clone());
            }
        }
        self
    }

    /// Enable auto gzip decompression by checking the ContentEncoding response header.
    ///
    /// Default is enabled.
    #[inline]
    pub fn gzip(&mut self, enable: bool) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.gzip = enable;
        }
        self
    }

    /// Add a `Proxy` to the list of proxies the `Client` will use.
    #[inline]
    pub fn proxy(&mut self, proxy: Proxy) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.proxies.push(proxy);
        }
        self
    }

    /// Set a `RedirectPolicy` for this client.
    ///
    /// Default will follow redirects up to a maximum of 10.
    #[inline]
    pub fn redirect(&mut self, policy: RedirectPolicy) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.redirect_policy = policy;
        }
        self
    }

    /// Enable or disable automatic setting of the `Referer` header.
    ///
    /// Default is `true`.
    #[inline]
    pub fn referer(&mut self, enable: bool) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.referer = enable;
        }
        self
    }

    /// Set a timeout for both the read and write operations of a client.
    #[inline]
    pub fn timeout(&mut self, timeout: Duration) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.timeout = Some(timeout);
        }
        self
    }

    /// Set number of DNS threads.
    #[inline]
    pub fn dns_threads(&mut self, threads: usize) -> &mut ClientBuilder {
        if let Some(config) = config_mut(&mut self.config, &self.err) {
            config.dns_threads = threads;
        }
        self
    }
}

fn config_mut<'a>(config: &'a mut Option<Config>, err: &Option<::Error>) -> Option<&'a mut Config> {
    if err.is_some() {
        None
    } else {
        config.as_mut()
    }
}

type HyperClient = ::hyper::Client<Connector>;

impl Client {
    /// Constructs a new `Client`.
    ///
    /// # Panics
    ///
    /// This method panics if native TLS backend cannot be created or
    /// initialized. Use `Client::builder()` if you wish to handle the failure
    /// as an `Error` instead of panicking.
    #[inline]
    pub fn new() -> Client {
        ClientBuilder::new()
            .build()
            .expect("TLS failed to initialize")
    }

    /// Creates a `ClientBuilder` to configure a `Client`.
    #[inline]
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
        let req = match url.into_url() {
            Ok(url) => Ok(Request::new(method, url)),
            Err(err) => Err(::error::from(err)),
        };
        request::builder(self.clone(), req)
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
    pub fn execute(&self, request: Request) -> Pending {
        self.execute_request(request)
    }


    fn execute_request(&self, req: Request) -> Pending {
        let (
            method,
            url,
            user_headers,
            body
        ) = request::pieces(req);

        let mut headers = self.inner.headers.clone(); // default headers
        for (key, value) in user_headers.iter() {
            headers.insert(key, value.clone());
        }

        if self.inner.gzip &&
            !headers.contains_key(ACCEPT_ENCODING) &&
            !headers.contains_key(RANGE) {
            headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        }

        let uri = to_uri(&url);

        let (reusable, body) = match body {
            Some(body) => {
                let (reusable, body) = body::into_hyper(body);
                (Some(reusable), body)
            },
            None => {
                (None, ::hyper::Body::empty())
            }
        };

        let mut req = ::hyper::Request::builder()
            .method(method.clone())
            .uri(uri.clone())
            .body(body)
            .expect("valid request parts");

        *req.headers_mut() = headers.clone();

        let in_flight = self.inner.hyper.request(req);

        Pending {
            inner: PendingInner::Request(PendingRequest {
                method: method,
                url: url,
                headers: headers,
                body: reusable,

                urls: Vec::new(),

                client: self.inner.clone(),

                in_flight: in_flight,
            }),
        }
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
            .field("gzip", &self.inner.gzip)
            .field("redirect_policy", &self.inner.redirect_policy)
            .field("referer", &self.inner.referer)
            .finish()
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .finish()
    }
}

struct ClientRef {
    gzip: bool,
    headers: HeaderMap,
    hyper: HyperClient,
    proxies: Arc<Vec<Proxy>>,
    redirect_policy: RedirectPolicy,
    referer: bool,
}

pub struct Pending {
    inner: PendingInner,
}

enum PendingInner {
    Request(PendingRequest),
    Error(Option<::Error>),
}

pub struct PendingRequest {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Option<Bytes>>,

    urls: Vec<Url>,

    client: Arc<ClientRef>,

    in_flight: ResponseFuture,
}


impl Future for Pending {
    type Item = Response;
    type Error = ::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner {
            PendingInner::Request(ref mut req) => req.poll(),
            PendingInner::Error(ref mut err) => Err(err.take().expect("Pending error polled more than once")),
        }
    }
}

impl Future for PendingRequest {
    type Item = Response;
    type Error = ::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let res = match try_!(self.in_flight.poll(), &self.url) {
                Async::Ready(res) => res,
                Async::NotReady => return Ok(Async::NotReady),
            };
            let should_redirect = match res.status() {
                StatusCode::MOVED_PERMANENTLY |
                StatusCode::FOUND |
                StatusCode::SEE_OTHER => {
                    self.body = None;
                    match self.method {
                        Method::GET | Method::HEAD => {},
                        _ => {
                            self.method = Method::GET;
                        }
                    }
                    true
                },
                StatusCode::TEMPORARY_REDIRECT |
                StatusCode::PERMANENT_REDIRECT => match self.body {
                    Some(Some(_)) | None => true,
                    Some(None) => false,
                },
                _ => false,
            };
            if should_redirect {
                let loc = res.headers()
                    .get(LOCATION)
                    .map(|loc| self.url.join(loc.to_str().expect("")));
                if let Some(Ok(loc)) = loc {
                    if self.client.referer {
                        if let Some(referer) = make_referer(&loc, &self.url) {
                            self.headers.insert(REFERER, referer);
                        }
                    }
                    self.urls.push(self.url.clone());
                    let action = check_redirect(
                        &self.client.redirect_policy,
                        res.status(),
                        &loc,
                        &self.urls,
                    );

                    match action {
                        redirect::Action::Follow => {
                            self.url = loc;

                            remove_sensitive_headers(&mut self.headers, &self.url, &self.urls);
                            debug!("redirecting to {:?} '{}'", self.method, self.url);
                            let uri = to_uri(&self.url);
                            let body = match self.body {
                                Some(Some(ref body)) => ::hyper::Body::from(body.clone()),
                                _ => ::hyper::Body::empty(),
                            };
                            let mut req = ::hyper::Request::builder()
                                .method(self.method.clone())
                                .uri(uri.clone())
                                .body(body)
                                .expect("valid request parts");

                            *req.headers_mut() = self.headers.clone();
                            self.in_flight = self.client.hyper.request(req);
                            continue;
                        },
                        redirect::Action::Stop => {
                            debug!("redirect_policy disallowed redirection to '{}'", loc);
                        },
                        redirect::Action::LoopDetected => {
                            return Err(::error::loop_detected(self.url.clone()));
                        },
                        redirect::Action::TooManyRedirects => {
                            return Err(::error::too_many_redirects(self.url.clone()));
                        }
                    }
                } else if let Some(Err(e)) = loc {
                    debug!("Location header had invalid URI: {:?}", e);
                }
            }
            let res = response::new(res, self.url.clone(), self.client.gzip);
            return Ok(Async::Ready(res));
        }
    }
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.inner {
            PendingInner::Request(ref req) => {
                f.debug_struct("Pending")
                    .field("method", &req.method)
                    .field("url", &req.url)
                    .finish()
            },
            PendingInner::Error(ref err) => {
                f.debug_struct("Pending")
                    .field("error", err)
                    .finish()
            }
        }
    }
}

fn make_referer(next: &Url, previous: &Url) -> Option<HeaderValue> {
    if next.scheme() == "http" && previous.scheme() == "https" {
        return None;
    }

    let mut referer = previous.clone();
    let _ = referer.set_username("");
    let _ = referer.set_password(None);
    referer.set_fragment(None);
    referer.as_str().parse().ok()
}

// pub(crate)

pub fn take_builder(builder: &mut ClientBuilder) -> ClientBuilder {
    use std::mem;
    mem::replace(builder, ClientBuilder { config: None, err: None })
}

pub fn pending_err(err: ::Error) -> Pending {
    Pending {
        inner: PendingInner::Error(Some(err)),
    }
}
