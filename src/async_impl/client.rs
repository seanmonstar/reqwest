use std::{fmt, str};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::net::IpAddr;

use bytes::Bytes;
use futures::{Async, Future, Poll};
use futures::future::Executor;
use header::{
    Entry,
    HeaderMap,
    HeaderValue,
    ACCEPT,
    ACCEPT_ENCODING,
    CONTENT_LENGTH,
    CONTENT_ENCODING,
    CONTENT_TYPE,
    LOCATION,
    PROXY_AUTHORIZATION,
    RANGE,
    REFERER,
    TRANSFER_ENCODING,
    USER_AGENT,
};
use http::Uri;
use hyper::client::ResponseFuture;
use mime;
#[cfg(feature = "default-tls")]
use native_tls::TlsConnector;
use tokio::{clock, timer::Delay};


use super::request::{Request, RequestBuilder};
use super::response::Response;
use connect::Connector;
use into_url::{expect_uri, try_uri};
use cookie;
use redirect::{self, RedirectPolicy, remove_sensitive_headers};
use {IntoUrl, Method, Proxy, StatusCode, Url};
use ::proxy::get_proxies;
#[cfg(feature = "tls")]
use {Certificate, Identity};
#[cfg(feature = "tls")]
use ::tls::TlsBackend;

static DEFAULT_USER_AGENT: &'static str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// An asynchronous `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value. To configure a
/// `Client`, use `Client::builder()`.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and **reuse** it.
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientRef>,
}

/// A `ClientBuilder` can be used to create a `Client` with  custom configuration.
pub struct ClientBuilder {
    config: Config,
}

struct Config {
    gzip: bool,
    headers: HeaderMap,
    #[cfg(feature = "default-tls")]
    hostname_verification: bool,
    #[cfg(feature = "tls")]
    certs_verification: bool,
    connect_timeout: Option<Duration>,
    #[cfg(feature = "tls")]
    identity: Option<Identity>,
    proxies: Vec<Proxy>,
    redirect_policy: RedirectPolicy,
    referer: bool,
    timeout: Option<Duration>,
    #[cfg(feature = "tls")]
    root_certs: Vec<Certificate>,
    #[cfg(feature = "tls")]
    tls: TlsBackend,
    http2_only: bool,
    local_address: Option<IpAddr>,
    nodelay: bool,
    cookie_store: Option<cookie::CookieStore>,
    http_builder: hyper::client::Builder,
}

impl ClientBuilder {
    /// Constructs a new `ClientBuilder`.
    ///
    /// This is the same as `Client::builder()`.
    pub fn new() -> ClientBuilder {
        let mut headers: HeaderMap<HeaderValue> = HeaderMap::with_capacity(2);
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_str(mime::STAR_STAR.as_ref()).expect("unable to parse mime"));

        ClientBuilder {
            config: Config {
                gzip: true,
                headers: headers,
                #[cfg(feature = "default-tls")]
                hostname_verification: true,
                #[cfg(feature = "tls")]
                certs_verification: true,
                connect_timeout: None,
                proxies: Vec::new(),
                redirect_policy: RedirectPolicy::default(),
                referer: true,
                timeout: None,
                #[cfg(feature = "tls")]
                root_certs: Vec::new(),
                #[cfg(feature = "tls")]
                identity: None,
                #[cfg(feature = "tls")]
                tls: TlsBackend::default(),
                http2_only: false,
                local_address: None,
                nodelay: false,
                cookie_store: None,
                http_builder: hyper::Client::builder(),
            },
        }
    }

    /// Returns a `Client` that uses this `ClientBuilder` configuration.
    ///
    /// # Errors
    ///
    /// This method fails if TLS backend cannot be initialized, or the resolver
    /// cannot load the system configuration.
    pub fn build(self) -> ::Result<Client> {
        let config = self.config;
        let proxies = Arc::new(config.proxies);

        let mut connector = {
            #[cfg(feature = "tls")]
            fn user_agent(headers: &HeaderMap) -> HeaderValue {
                headers[USER_AGENT].clone()
            }

            #[cfg(feature = "tls")]
            match config.tls {
                #[cfg(feature = "default-tls")]
                TlsBackend::Default => {
                    let mut tls = TlsConnector::builder();
                    tls.danger_accept_invalid_hostnames(!config.hostname_verification);
                    tls.danger_accept_invalid_certs(!config.certs_verification);

                    for cert in config.root_certs {
                        cert.add_to_native_tls(&mut tls);
                    }

                    if let Some(id) = config.identity {
                        id.add_to_native_tls(&mut tls)?;
                    }

                    Connector::new_default_tls(tls, proxies.clone(), user_agent(&config.headers), config.local_address, config.nodelay)?
                },
                #[cfg(feature = "rustls-tls")]
                TlsBackend::Rustls => {
                    use ::tls::NoVerifier;

                    let mut tls = ::rustls::ClientConfig::new();
                    if config.http2_only {
                        tls.set_protocols(&["h2".into()]);
                    } else {
                        tls.set_protocols(&[
                            "h2".into(),
                            "http/1.1".into(),
                        ]);
                    }
                    tls.root_store.add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);

                    if !config.certs_verification {
                        tls.dangerous().set_certificate_verifier(Arc::new(NoVerifier));
                    }

                    for cert in config.root_certs {
                        cert.add_to_rustls(&mut tls)?;
                    }

                    if let Some(id) = config.identity {
                        id.add_to_rustls(&mut tls)?;
                    }

                    Connector::new_rustls_tls(tls, proxies.clone(), user_agent(&config.headers), config.local_address, config.nodelay)?
                }
            }

            #[cfg(not(feature = "tls"))]
            Connector::new(proxies.clone(), config.local_address, config.nodelay)?
        };

        connector.set_timeout(config.connect_timeout);

        let hyper_client = config.http_builder.build(connector);

        let proxies_maybe_http_auth = proxies
            .iter()
            .any(|p| p.maybe_has_http_auth());

        let cookie_store = config.cookie_store.map(RwLock::new);

        Ok(Client {
            inner: Arc::new(ClientRef {
                cookie_store,
                gzip: config.gzip,
                hyper: hyper_client,
                headers: config.headers,
                redirect_policy: config.redirect_policy,
                referer: config.referer,
                request_timeout: config.timeout,
                proxies,
                proxies_maybe_http_auth,
            }),
        })
    }

    /// Set that all sockets have `SO_NODELAY` set to `true`.
    pub fn tcp_nodelay(mut self) -> ClientBuilder {
        self.config.nodelay = true;
        self
    }

    /// Use native TLS backend.
    #[cfg(feature = "default-tls")]
    pub fn use_default_tls(mut self) -> ClientBuilder {
        self.config.tls = TlsBackend::Default;
        self
    }

    /// Use rustls TLS backend.
    #[cfg(feature = "rustls-tls")]
    pub fn use_rustls_tls(mut self) -> ClientBuilder {
        self.config.tls = TlsBackend::Rustls;
        self
    }

    /// Add a custom root certificate.
    ///
    /// This can be used to connect to a server that has a self-signed
    /// certificate for example.
    #[cfg(feature = "tls")]
    pub fn add_root_certificate(mut self, cert: Certificate) -> ClientBuilder {
        self.config.root_certs.push(cert);
        self
    }

    /// Sets the identity to be used for client certificate authentication.
    #[cfg(feature = "tls")]
    pub fn identity(mut self, identity: Identity) -> ClientBuilder {
        self.config.identity = Some(identity);
        self
    }

    /// Controls the use of hostname verification.
    ///
    /// Defaults to `false`.
    ///
    /// # Warning
    ///
    /// You should think very carefully before you use this method. If
    /// hostname verification is not used, any valid certificate for any
    /// site will be trusted for use from any other. This introduces a
    /// significant vulnerability to man-in-the-middle attacks.
    #[cfg(feature = "default-tls")]
    pub fn danger_accept_invalid_hostnames(mut self, accept_invalid_hostname: bool) -> ClientBuilder {
        self.config.hostname_verification = !accept_invalid_hostname;
        self
    }

    /// Controls the use of certificate validation.
    ///
    /// Defaults to `false`.
    ///
    /// # Warning
    ///
    /// You should think very carefully before using this method. If
    /// invalid certificates are trusted, *any* certificate for *any* site
    /// will be trusted for use. This includes expired certificates. This
    /// introduces significant vulnerabilities, and should only be used
    /// as a last resort.
    #[cfg(feature = "tls")]
    pub fn danger_accept_invalid_certs(mut self, accept_invalid_certs: bool) -> ClientBuilder {
        self.config.certs_verification = !accept_invalid_certs;
        self
    }


    /// Sets the default headers for every request.
    pub fn default_headers(mut self, headers: HeaderMap) -> ClientBuilder {
        for (key, value) in headers.iter() {
            self.config.headers.insert(key, value.clone());
        }
        self
    }

    /// Enable auto gzip decompression by checking the ContentEncoding response header.
    ///
    /// If auto gzip decompresson is turned on:
    /// - When sending a request and if the request's headers do not already contain
    ///   an `Accept-Encoding` **and** `Range` values, the `Accept-Encoding` header is set to `gzip`.
    ///   The body is **not** automatically inflated.
    /// - When receiving a response, if it's headers contain a `Content-Encoding` value that
    ///   equals to `gzip`, both values `Content-Encoding` and `Content-Length` are removed from the
    ///   headers' set. The body is automatically deinflated.
    ///
    /// Default is enabled.
    pub fn gzip(mut self, enable: bool) -> ClientBuilder {
        self.config.gzip = enable;
        self
    }

    /// Add a `Proxy` to the list of proxies the `Client` will use.
    pub fn proxy(mut self, proxy: Proxy) -> ClientBuilder {
        self.config.proxies.push(proxy);
        self
    }

    /// Clear all `Proxies`, so `Client` will use no proxy anymore.
    pub fn no_proxy(mut self) -> ClientBuilder {
        self.config.proxies.clear();
        self
    }

    /// Add system proxy setting to the list of proxies
    pub fn use_sys_proxy(mut self) -> ClientBuilder {
        let proxies = get_proxies();
        self.config.proxies.push(Proxy::custom(move |url| {
            return if proxies.contains_key(url.scheme()) {
                Some((*proxies.get(url.scheme()).unwrap()).clone())
            } else {
                None
            }
        }));
        self
    }


    /// Set a `RedirectPolicy` for this client.
    ///
    /// Default will follow redirects up to a maximum of 10.
    pub fn redirect(mut self, policy: RedirectPolicy) -> ClientBuilder {
        self.config.redirect_policy = policy;
        self
    }

    /// Enable or disable automatic setting of the `Referer` header.
    ///
    /// Default is `true`.
    pub fn referer(mut self, enable: bool) -> ClientBuilder {
        self.config.referer = enable;
        self
    }

    /// Enables a request timeout.
    ///
    /// The timeout is applied from the when the request starts connecting
    /// until the response body has finished.
    ///
    /// Default is no timeout.
    pub fn timeout(mut self, timeout: Duration) -> ClientBuilder {
        self.config.timeout = Some(timeout);
        self
    }

    /// Sets the maximum idle connection per host allowed in the pool.
    ///
    /// Default is usize::MAX (no limit).
    pub fn max_idle_per_host(mut self, max: usize) -> ClientBuilder {
        self.config.http_builder.max_idle_per_host(max);
        self
    }

    /// Only use HTTP/2.
    pub fn h2_prior_knowledge(mut self) -> ClientBuilder {
        self.config.http2_only = true;
        self.config.http_builder.http2_only(true);
        self
    }

    /// Enable case sensitive headers.
    pub fn http1_title_case_headers(mut self) -> ClientBuilder {
        self.config.http_builder.http1_title_case_headers(true);
        self
    }

    /// Allow changing the Hyper runtime executor
    pub fn executor<E>(mut self, executor: E) -> ClientBuilder
    where
        E: Executor<Box<dyn Future<Item=(), Error=()> + Send>> + Send + Sync + 'static
    {
        self.config.http_builder.executor(executor);
        self
    }

    /// Set a timeout for only the connect phase of a `Client`.
    ///
    /// Default is `None`.
    ///
    /// # Note
    ///
    /// This **requires** the futures be executed in a tokio runtime with
    /// a tokio timer enabled.
    pub fn connect_timeout(mut self, timeout: Duration) -> ClientBuilder {
        self.config.connect_timeout = Some(timeout);
        self
    }

    #[doc(hidden)]
    #[deprecated(note = "DNS no longer uses blocking threads")]
    pub fn dns_threads(self, _threads: usize) -> ClientBuilder {
        self
    }

    /// Bind to a local IP Address
    pub fn local_address<T>(mut self, addr: T) -> ClientBuilder
    where
        T: Into<Option<IpAddr>>,
    {
        self.config.local_address = addr.into();
        self
    }

    /// Enable a persistent cookie store for the client.
    ///
    /// Cookies received in responses will be preserved and included in
    /// additional requests.
    ///
    /// By default, no cookie store is used.
    pub fn cookie_store(mut self, enable: bool) -> ClientBuilder {
        self.config.cookie_store = if enable {
            Some(cookie::CookieStore::default())
        } else {
            None
        };
        self
    }
}

type HyperClient = ::hyper::Client<Connector>;

impl Client {
    /// Constructs a new `Client`.
    ///
    /// # Panics
    ///
    /// This method panics if TLS backend cannot initialized, or the resolver
    /// cannot load the system configuration.
    ///
    /// Use `Client::builder()` if you wish to handle the failure as an `Error`
    /// instead of panicking.
    pub fn new() -> Client {
        ClientBuilder::new()
            .build()
            .expect("Client::new()")
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
        let req = url
            .into_url()
            .map(move |url| Request::new(method, url));
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
    pub fn execute(&self, request: Request) -> impl Future<Item = Response, Error = ::Error> {
        self.execute_request(request)
    }


    pub(super) fn execute_request(&self, req: Request) -> Pending {
        let (
            method,
            url,
            mut headers,
            body
        ) = req.pieces();

        // insert default headers in the request headers
        // without overwriting already appended headers.
        for (key, value) in &self.inner.headers {
            if let Ok(Entry::Vacant(entry)) = headers.entry(key) {
                entry.insert(value.clone());
            }
        }

        // Add cookies from the cookie store.
        if let Some(cookie_store_wrapper) = self.inner.cookie_store.as_ref() {
            if headers.get(::header::COOKIE).is_none() {
                let cookie_store = cookie_store_wrapper.read().unwrap();
                add_cookie_header(&mut headers, &cookie_store, &url);
            }
        }

        if self.inner.gzip &&
            !headers.contains_key(ACCEPT_ENCODING) &&
            !headers.contains_key(RANGE) {
            headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        }

        let uri = expect_uri(&url);

        let (reusable, body) = match body {
            Some(body) => {
                let (reusable, body) = body.into_hyper();
                (Some(reusable), body)
            },
            None => {
                (None, ::hyper::Body::empty())
            }
        };

        self.proxy_auth(&uri, &mut headers);

        let mut req = ::hyper::Request::builder()
            .method(method.clone())
            .uri(uri.clone())
            .body(body)
            .expect("valid request parts");

        *req.headers_mut() = headers.clone();

        let in_flight = self.inner.hyper.request(req);

        let timeout = self.inner.request_timeout.map(|dur| {
            Delay::new(clock::now() + dur)
        });

        Pending {
            inner: PendingInner::Request(PendingRequest {
                method: method,
                url: url,
                headers: headers,
                body: reusable,

                urls: Vec::new(),

                client: self.inner.clone(),

                in_flight: in_flight,
                timeout,
            }),
        }
    }

    fn proxy_auth(&self, dst: &Uri, headers: &mut HeaderMap) {
        if !self.inner.proxies_maybe_http_auth {
            return;
        }

        // Only set the header here if the destination scheme is 'http',
        // since otherwise, the header will be included in the CONNECT tunnel
        // request instead.
        if dst.scheme_part() != Some(&::http::uri::Scheme::HTTP) {
            return;
        }

        if headers.contains_key(PROXY_AUTHORIZATION) {
            return;
        }


        for proxy in self.inner.proxies.iter() {
            if proxy.is_match(dst) {
                match proxy.http_basic_auth(dst) {
                    Some(header) => {
                        headers.insert(
                            PROXY_AUTHORIZATION,
                            header,
                        );
                    },
                    None => (),
                }

                break;
            }
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
    cookie_store: Option<RwLock<cookie::CookieStore>>,
    gzip: bool,
    headers: HeaderMap,
    hyper: HyperClient,
    redirect_policy: RedirectPolicy,
    referer: bool,
    request_timeout: Option<Duration>,
    proxies: Arc<Vec<Proxy>>,
    proxies_maybe_http_auth: bool,
}

pub(super) struct Pending {
    inner: PendingInner,
}

enum PendingInner {
    Request(PendingRequest),
    Error(Option<::Error>),
}

struct PendingRequest {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Option<Bytes>>,

    urls: Vec<Url>,

    client: Arc<ClientRef>,

    in_flight: ResponseFuture,
    timeout: Option<Delay>,
}

impl Pending {
    pub(super) fn new_err(err: ::Error) -> Pending {
        Pending {
            inner: PendingInner::Error(Some(err)),
        }
    }
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
        if let Some(ref mut delay) = self.timeout {
            if let Async::Ready(()) = try_!(delay.poll(), &self.url) {
                return Err(::error::timedout(Some(self.url.clone())));
            }
        }

        loop {
            let res = match try_!(self.in_flight.poll(), &self.url) {
                Async::Ready(res) => res,
                Async::NotReady => return Ok(Async::NotReady),
            };
            if let Some(store_wrapper) = self.client.cookie_store.as_ref() {
                let mut store = store_wrapper.write().unwrap();
                let cookies = cookie::extract_response_cookies(&res.headers())
                    .filter_map(|res| res.ok())
                    .map(|cookie| cookie.into_inner().into_owned());
                store.0.store_response_cookies(cookies, &self.url);
            }
            let should_redirect = match res.status() {
                StatusCode::MOVED_PERMANENTLY |
                StatusCode::FOUND |
                StatusCode::SEE_OTHER => {
                    self.body = None;
                    for header in &[TRANSFER_ENCODING, CONTENT_ENCODING, CONTENT_TYPE, CONTENT_LENGTH] {
                        self.headers.remove(header);
                    }

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
                    .and_then(|val| {
                        let loc = (|| -> Option<Url> {
                            // Some sites may send a utf-8 Location header,
                            // even though we're supposed to treat those bytes
                            // as opaque, we'll check specifically for utf8.
                            self.url.join(str::from_utf8(val.as_bytes()).ok()?).ok()
                        })();

                        // Check that the `url` is also a valid `http::Uri`.
                        //
                        // If not, just log it and skip the redirect.
                        let loc = loc.and_then(|url| {
                            if try_uri(&url).is_some() {
                                Some(url)
                            } else {
                                None
                            }
                        });

                        if loc.is_none() {
                            debug!("Location header had invalid URI: {:?}", val);
                        }
                        loc
                    });
                if let Some(loc) = loc {
                    if self.client.referer {
                        if let Some(referer) = make_referer(&loc, &self.url) {
                            self.headers.insert(REFERER, referer);
                        }
                    }
                    self.urls.push(self.url.clone());
                    let action = self.client.redirect_policy.check(
                        res.status(),
                        &loc,
                        &self.urls,
                    );

                    match action {
                        redirect::Action::Follow => {
                            self.url = loc;

                            remove_sensitive_headers(&mut self.headers, &self.url, &self.urls);
                            debug!("redirecting to {:?} '{}'", self.method, self.url);
                            let uri = expect_uri(&self.url);
                            let body = match self.body {
                                Some(Some(ref body)) => ::hyper::Body::from(body.clone()),
                                _ => ::hyper::Body::empty(),
                            };
                            let mut req = ::hyper::Request::builder()
                                .method(self.method.clone())
                                .uri(uri.clone())
                                .body(body)
                                .expect("valid request parts");

                            // Add cookies from the cookie store.
                            if let Some(cookie_store_wrapper) = self.client.cookie_store.as_ref() {
                                let cookie_store = cookie_store_wrapper.read().unwrap();
                                add_cookie_header(&mut self.headers, &cookie_store, &self.url);
                            }

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
                }
            }
            let res = Response::new(res, self.url.clone(), self.client.gzip, self.timeout.take());
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

fn add_cookie_header(headers: &mut HeaderMap, cookie_store: &cookie::CookieStore, url: &Url) {
    let header = cookie_store
        .0
        .get_request_cookies(url)
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");
    if !header.is_empty() {
        headers.insert(
            ::header::COOKIE,
            HeaderValue::from_bytes(header.as_bytes()).unwrap()
        );
    }
}
