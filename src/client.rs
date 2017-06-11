use std::fmt;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use hyper::client::IntoUrl;
use hyper::header::{Location, Referer, UserAgent, Accept, Encoding, AcceptEncoding, Range, qitem};
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::Url;

use hyper_native_tls::{NativeTlsClient, TlsStream, native_tls};

use body;
use redirect::{self, RedirectPolicy, check_redirect, remove_sensitive_headers};
use request::{self, Request, RequestBuilder, MultipartRequestBuilder};
use response::Response;

static DEFAULT_USER_AGENT: &'static str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// A `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and reuse it.
///
/// # Examples
///
/// ```rust
/// # use reqwest::{Error, Client};
/// #
/// # fn run() -> Result<(), Error> {
/// let client = Client::new()?;
/// let resp = client.get("http://httpbin.org/")?.send()?;
/// #   drop(resp);
/// #   Ok(())
/// # }
///
/// ```
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientRef>,
}

/// Represent an X509 certificate.
pub struct Certificate(native_tls::Certificate);

impl Certificate {
    /// Create a `Certificate` from a binary DER encoded certificate
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn cert() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my_cert.der")?
    ///     .read_to_end(&mut buf)?;
    /// let cert = reqwest::Certificate::from_der(&buf)?;
    /// # drop(cert);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// If the provided buffer is not valid DER, an error will be returned.
    pub fn from_der(der: &[u8]) -> ::Result<Certificate> {
        let inner = try_!(native_tls::Certificate::from_der(der));
        Ok(Certificate(inner))
    }
}

/// A `ClientBuilder` can be used to create a `Client` with  custom configuration:
///
/// - with hostname verification disabled
/// - with one or multiple custom certificates
///
/// # Examples
///
/// ```
/// # use std::fs::File;
/// # use std::io::Read;
/// # fn build_client() -> Result<(), Box<std::error::Error>> {
/// // read a local binary DER encoded certificate
/// let mut buf = Vec::new();
/// File::open("my-cert.der")?.read_to_end(&mut buf)?;
///
/// // create a certificate
/// let cert = reqwest::Certificate::from_der(&buf)?;
///
/// // get a client builder
/// let client = reqwest::ClientBuilder::new()?
///     .add_root_certificate(cert)?
///     .build()?;
/// # drop(client);
/// # Ok(())
/// # }
/// ```
pub struct ClientBuilder {
    config: Option<Config>,
}

struct Config {
    gzip: bool,
    hostname_verification: bool,
    redirect_policy: RedirectPolicy,
    referer: bool,
    timeout: Option<Duration>,
    tls: native_tls::TlsConnectorBuilder,
}

impl ClientBuilder {
    /// Constructs a new `ClientBuilder`
    ///
    /// # Errors
    ///
    /// This method fails if native TLS backend cannot be created.
    pub fn new() -> ::Result<ClientBuilder> {
        let tls_connector_builder = try_!(native_tls::TlsConnector::builder());
        Ok(ClientBuilder {
           config: Some(Config {
                gzip: true,
                hostname_verification: true,
                redirect_policy: RedirectPolicy::default(),
                referer: true,
                timeout: None,
                tls: tls_connector_builder,
            }),
       })
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
        let config = self.take_config();

        let tls_connector = try_!(config.tls.build());
        let mut tls_client = NativeTlsClient::from(tls_connector);
        if !config.hostname_verification {
            tls_client.danger_disable_hostname_verification(true);
        }

        let mut hyper_client = create_hyper_client(tls_client);

        hyper_client.set_read_timeout(config.timeout);
        hyper_client.set_write_timeout(config.timeout);

        Ok(Client {
           inner: Arc::new(ClientRef {
               gzip: config.gzip,
               hyper: hyper_client,
               redirect_policy: config.redirect_policy,
               referer: config.referer,
           }),
       })
    }

    /// Add a custom root certificate.
    ///
    /// This can be used to connect to a server that has a self-signed
    /// certificate for example.
    ///
    /// # Errors
    ///
    /// This method fails if adding root certificate was unsuccessful.
    pub fn add_root_certificate(&mut self, cert: Certificate) -> ::Result<&mut ClientBuilder> {
        try_!(self.config_mut().tls.add_root_certificate(cert.0));
        Ok(self)
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
        self.config_mut().hostname_verification = false;
        self
    }

    /// Enable hostname verification.
    #[inline]
    pub fn enable_hostname_verification(&mut self) -> &mut ClientBuilder {
        self.config_mut().hostname_verification = true;
        self
    }

    /// Enable auto gzip decompression by checking the ContentEncoding response header.
    ///
    /// Default is enabled.
    #[inline]
    pub fn gzip(&mut self, enable: bool) -> &mut ClientBuilder {
        self.config_mut().gzip = enable;
        self
    }

    /// Set a `RedirectPolicy` for this client.
    ///
    /// Default will follow redirects up to a maximum of 10.
    #[inline]
    pub fn redirect(&mut self, policy: RedirectPolicy) -> &mut ClientBuilder {
        self.config_mut().redirect_policy = policy;
        self
    }

    /// Enable or disable automatic setting of the `Referer` header.
    ///
    /// Default is `true`.
    #[inline]
    pub fn referer(&mut self, enable: bool) -> &mut ClientBuilder {
        self.config_mut().referer = enable;
        self
    }

    /// Set a timeout for both the read and write operations of a client.
    #[inline]
    pub fn timeout(&mut self, timeout: Duration) -> &mut ClientBuilder {
        self.config_mut().timeout = Some(timeout);
        self
    }

    // private
    fn config_mut(&mut self) -> &mut Config {
        self.config
            .as_mut()
            .expect("ClientBuilder cannot be reused after building a Client")
    }

    fn take_config(&mut self) -> Config {
        self.config
            .take()
            .expect("ClientBuilder cannot be reused after building a Client")
    }
}

fn create_hyper_client(tls_client: NativeTlsClient) -> ::hyper::Client {
    let mut pool = ::hyper::client::Pool::with_connector(
        Default::default(),
        ::hyper::net::HttpsConnector::new(tls_client),
    );
    // For now, while experiementing, they're constants.
    // TODO: maybe make these configurable someday?
    pool.set_idle_timeout(Some(Duration::from_secs(60 * 2)));
    pool.set_stale_check(|mut check| {
        if stream_dead(check.stream()) {
            check.stale()
        } else {
            check.fresh()
        }
    });

    let mut hyper_client = ::hyper::Client::with_connector(pool);

    hyper_client.set_redirect_policy(::hyper::client::RedirectPolicy::FollowNone);
    hyper_client
}

fn stream_dead(stream: &::hyper::net::HttpsStream<TlsStream<::hyper::net::HttpStream>>) -> bool {
    match *stream {
        ::hyper::net::HttpsStream::Http(ref http) => socket_is_dead(&http.0),
        ::hyper::net::HttpsStream::Https(ref https) => socket_is_dead(&https.lock().get_ref().0),
    }
}

#[cfg(unix)]
fn socket_is_dead(socket: &TcpStream) -> bool {
    use std::mem;
    use std::os::unix::io::AsRawFd;
    use std::ptr;
    use libc::{FD_SET, select, timeval};

    let ret = unsafe {
        let fd = socket.as_raw_fd();
        let nfds = fd + 1;

        let mut timeout = timeval {
            tv_sec: 0,
            tv_usec: 0,
        };

        let mut readfs = mem::zeroed();
        let mut errfs = mem::zeroed();
        FD_SET(fd, &mut readfs);
        FD_SET(fd, &mut errfs);
        select(nfds, &mut readfs, ptr::null_mut(), &mut errfs, &mut timeout)
    };

    // socket was readable (eof), or an error, then it's dead
    ret != 0
}

#[cfg(windows)]
fn socket_is_dead(socket: &TcpStream) -> bool {
    use std::mem;
    use std::os::windows::io::{AsRawSocket, RawSocket};
    use std::ptr;
    use libc::{c_int, timeval};

    const FD_SETSIZE: usize = 64;

    #[repr(C)]
    struct fd_set {
        fd_count: c_int,
        fd_array: [RawSocket; FD_SETSIZE],
    }

    extern "system" {
        fn select(maxfds: c_int, readfs: *mut fd_set, writefs: *mut fd_set,
                  errfs: *mut fd_set, timeout: *mut timeval) -> c_int;
    }

    let ret = unsafe {
        let fd = socket.as_raw_socket();
        let nfds = 0; // msdn says nfds is ignored
        let mut timeout = timeval {
            tv_sec: 0,
            tv_usec: 0,
        };

        let mut readfs: fd_set = mem::zeroed();
        let mut errfs: fd_set = mem::zeroed();
        readfs.fd_count = 1;
        readfs.fd_array[0] = fd;
        errfs.fd_count = 1;
        errfs.fd_array[0] = fd;

        select(nfds, &mut readfs, ptr::null_mut(), &mut errfs, &mut timeout)
    };

    // socket was readable (eof), or an error, then it's dead
    ret != 0
}

impl Client {
    /// Constructs a new `Client`.
    ///
    /// # Errors
    ///
    /// This method fails if native TLS backend cannot be created or initialized.
    #[inline]
    pub fn new() -> ::Result<Client> {
        ClientBuilder::new()?.build()
    }

    /// Creates a `ClientBuilder` to configure a `Client`.
    ///
    /// # Errors
    ///
    /// This method fails if native TLS backend cannot be created.
    #[inline]
    pub fn builder() -> ::Result<ClientBuilder> {
        ClientBuilder::new()
    }

    /// Convenience method to make a `GET` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn get<U: IntoUrl>(&self, url: U) -> ::Result<RequestBuilder> {
        self.request(Method::Get, url)
    }

    /// Convenience method to make a `POST` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn post<U: IntoUrl>(&self, url: U) -> ::Result<RequestBuilder> {
        self.request(Method::Post, url)
    }

    /// Convenience method to make a `PUT` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn put<U: IntoUrl>(&self, url: U) -> ::Result<RequestBuilder> {
        self.request(Method::Put, url)
    }

    /// Convenience method to make a `PATCH` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn patch<U: IntoUrl>(&self, url: U) -> ::Result<RequestBuilder> {
        self.request(Method::Patch, url)
    }

    /// Convenience method to make a `DELETE` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn delete<U: IntoUrl>(&self, url: U) -> ::Result<RequestBuilder> {
        self.request(Method::Delete, url)
    }

    /// Convenience method to make a `HEAD` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn head<U: IntoUrl>(&self, url: U) -> ::Result<RequestBuilder> {
        self.request(Method::Head, url)
    }

    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// request body before sending.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> ::Result<RequestBuilder> {
        let url = try_!(url.into_url());
        Ok(request::builder(self.clone(), Request::new(method, url)))
    }
    /// Start building a multipart/form-data `Request` with the `Url`.
    ///
    /// Returns a `MultipartRequestBuilder`, which will allow setting headers,
    /// request body, files and parameters before sending.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn multipart<U: IntoUrl>(&self, url: U) -> ::Result<MultipartRequestBuilder> {
        let url = try_!(url.into_url());
        let request_builder = request::builder(self.clone(), Request::new(Method::Post, url));
        Ok(MultipartRequestBuilder::new(request_builder))
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
    pub fn execute(&self, request: Request) -> ::Result<Response> {
        self.inner.execute_request(request)
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

struct ClientRef {
    gzip: bool,
    hyper: ::hyper::Client,
    redirect_policy: RedirectPolicy,
    referer: bool,
}

impl ClientRef {
    fn execute_request(&self, req: Request) -> ::Result<Response> {
        let (mut method,
             mut url,
             mut headers,
             mut body
        ) = request::pieces(req);

        if !headers.has::<UserAgent>() {
            headers.set(UserAgent(DEFAULT_USER_AGENT.to_owned()));
        }

        if !headers.has::<Accept>() {
            headers.set(Accept::star());
        }
        if self.gzip && !headers.has::<AcceptEncoding>() && !headers.has::<Range>() {
            headers.set(AcceptEncoding(vec![qitem(Encoding::Gzip)]));
        }

        let mut urls = Vec::new();

        loop {
            let res = {
                info!("Request: {:?} {}", method, url);
                let mut req = self.hyper .request(method.clone(), url.clone())
                    .headers(headers.clone());

                if let Some(ref mut b) = body {
                    let body = body::as_hyper_body(b);
                    req = req.body(body);
                }

                try_!(req.send(), &url)
            };

            let should_redirect = match res.status {
                StatusCode::MovedPermanently |
                StatusCode::Found |
                StatusCode::SeeOther => {
                    body = None;
                    match method {
                        Method::Get | Method::Head => {}
                        _ => {
                            method = Method::Get;
                        }
                    }
                    true
                },
                StatusCode::TemporaryRedirect |
                StatusCode::PermanentRedirect => {
                    if let Some(ref body) = body {
                        body::can_reset(body)
                    } else {
                        true
                    }
                },
                _ => false,
            };

            if should_redirect {
                let loc = {
                    let loc = res.headers.get::<Location>().map(|loc| url.join(loc));
                    if let Some(loc) = loc {
                        loc
                    } else {
                        return Ok(::response::new(res, self.gzip));
                    }
                };

                url = match loc {
                    Ok(loc) => {
                        if self.referer {
                            if let Some(referer) = make_referer(&loc, &url) {
                                headers.set(referer);
                            }
                        }
                        urls.push(url);
                        let action = check_redirect(&self.redirect_policy, &loc, &urls);

                        match action {
                            redirect::Action::Follow => loc,
                            redirect::Action::Stop => {
                                debug!("redirect_policy disallowed redirection to '{}'", loc);
                                return Ok(::response::new(res, self.gzip));
                            }
                            redirect::Action::LoopDetected => {
                                return Err(::error::loop_detected(res.url.clone()));
                            }
                            redirect::Action::TooManyRedirects => {
                                return Err(::error::too_many_redirects(res.url.clone()));
                            }
                        }
                    },
                    Err(e) => {
                        debug!("Location header had invalid URI: {:?}", e);

                        return Ok(::response::new(res, self.gzip));
                    }
                };

                remove_sensitive_headers(&mut headers, &url, &urls);
                debug!("redirecting to {:?} '{}'", method, url);
            } else {
                return Ok(::response::new(res, self.gzip));
            }
        }
    }
}

fn make_referer(next: &Url, previous: &Url) -> Option<Referer> {
    if next.scheme() == "http" && previous.scheme() == "https" {
        return None;
    }

    let mut referer = previous.clone();
    let _ = referer.set_username("");
    let _ = referer.set_password(None);
    referer.set_fragment(None);
    Some(Referer(referer.into_string()))
}
