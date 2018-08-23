use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use std::thread;

use futures::{Future, Stream};
use futures::sync::{mpsc, oneshot};

use request::{Request, RequestBuilder};
use response::{self, Response};
use {async_impl, header, Certificate, Identity, Method, IntoUrl, Proxy, RedirectPolicy, wait};

/// A `Client` to make Requests with.
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
/// # use reqwest::{Error, Client};
/// #
/// # fn run() -> Result<(), Error> {
/// let client = Client::new();
/// let resp = client.get("http://httpbin.org/").send()?;
/// #   drop(resp);
/// #   Ok(())
/// # }
///
/// ```
#[derive(Clone)]
pub struct Client {
    inner: ClientHandle,
}

/// A `ClientBuilder` can be used to create a `Client` with  custom configuration.
///
/// # Example
///
/// ```
/// # fn run() -> Result<(), reqwest::Error> {
/// use std::time::Duration;
///
/// let client = reqwest::Client::builder()
///     .gzip(true)
///     .timeout(Duration::from_secs(10))
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct ClientBuilder {
    inner: async_impl::ClientBuilder,
    timeout: Timeout,
}

impl ClientBuilder {
    /// Constructs a new `ClientBuilder`
    pub fn new() -> ClientBuilder {
        ClientBuilder {
            inner: async_impl::ClientBuilder::new(),
            timeout: Timeout::default(),
        }
    }

    /// Returns a `Client` that uses this `ClientBuilder` configuration.
    ///
    /// # Errors
    ///
    /// This method fails if native TLS backend cannot be initialized.
    pub fn build(self) -> ::Result<Client> {
        ClientHandle::new(self).map(|handle| Client {
            inner: handle,
        })
    }

    /// Add a custom root certificate.
    ///
    /// This can be used to connect to a server that has a self-signed
    /// certificate for example.
    ///
    /// # Example
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
    /// let client = reqwest::Client::builder()
    ///     .add_root_certificate(cert)
    ///     .build()?;
    /// # drop(client);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// This method fails if adding root certificate was unsuccessful.
    pub fn add_root_certificate(self, cert: Certificate) -> ClientBuilder {
        self.with_inner(move |inner| inner.add_root_certificate(cert))
    }

    /// Sets the identity to be used for client certificate authentication.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn build_client() -> Result<(), Box<std::error::Error>> {
    /// // read a local PKCS12 bundle
    /// let mut buf = Vec::new();
    /// File::open("my-ident.pfx")?.read_to_end(&mut buf)?;
    ///
    /// // create an Identity from the PKCS#12 archive
    /// let pkcs12 = reqwest::Identity::from_pkcs12_der(&buf, "my-privkey-password")?;
    ///
    /// // get a client builder
    /// let client = reqwest::Client::builder()
    ///     .identity(pkcs12)
    ///     .build()?;
    /// # drop(client);
    /// # Ok(())
    /// # }
    /// ```
    pub fn identity(self, identity: Identity) -> ClientBuilder {
        self.with_inner(move |inner| inner.identity(identity))
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
    #[inline]
    pub fn danger_accept_invalid_hostnames(self, accept_invalid_hostname: bool) -> ClientBuilder {
        self.with_inner(|inner| inner.danger_accept_invalid_hostnames(accept_invalid_hostname))
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
    #[inline]
    pub fn danger_accept_invalid_certs(self, accept_invalid_certs: bool) -> ClientBuilder {
        self.with_inner(|inner| inner.danger_accept_invalid_certs(accept_invalid_certs))
    }

    /// Sets the default headers for every request.
    ///
    /// # Example
    ///
    /// ```rust
    /// use reqwest::header;
    /// # fn build_client() -> Result<(), Box<std::error::Error>> {
    /// let mut headers = header::HeaderMap::new();
    /// headers.insert(header::AUTHORIZATION, header::HeaderValue::from_static("secret"));
    ///
    /// // get a client builder
    /// let client = reqwest::Client::builder()
    ///     .default_headers(headers)
    ///     .build()?;
    /// let res = client.get("https://www.rust-lang.org").send()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Override the default headers:
    ///
    /// ```rust
    /// use reqwest::header;
    /// # fn build_client() -> Result<(), Box<std::error::Error>> {
    /// let mut headers = header::HeaderMap::new();
    /// headers.insert(header::AUTHORIZATION, header::HeaderValue::from_static("secret"));
    ///
    /// // get a client builder
    /// let client = reqwest::Client::builder()
    ///     .default_headers(headers)
    ///     .build()?;
    /// let res = client
    ///     .get("https://www.rust-lang.org")
    ///     .header(header::AUTHORIZATION, "token")
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn default_headers(self, headers: header::HeaderMap) -> ClientBuilder {
        self.with_inner(move |inner| inner.default_headers(headers))
    }

    /// Enable auto gzip decompression by checking the ContentEncoding response header.
    ///
    /// Default is enabled.
    #[inline]
    pub fn gzip(self, enable: bool) -> ClientBuilder {
        self.with_inner(|inner| inner.gzip(enable))
    }

    /// Add a `Proxy` to the list of proxies the `Client` will use.
    #[inline]
    pub fn proxy(self, proxy: Proxy) -> ClientBuilder {
        self.with_inner(move |inner| inner.proxy(proxy))
    }

    /// Set a `RedirectPolicy` for this client.
    ///
    /// Default will follow redirects up to a maximum of 10.
    #[inline]
    pub fn redirect(self, policy: RedirectPolicy) -> ClientBuilder {
        self.with_inner(move |inner| inner.redirect(policy))
    }

    /// Enable or disable automatic setting of the `Referer` header.
    ///
    /// Default is `true`.
    #[inline]
    pub fn referer(self, enable: bool) -> ClientBuilder {
        self.with_inner(|inner| inner.referer(enable))
    }

    /// Set a timeout for connect, read and write operations of a `Client`.
    ///
    /// Default is 30 seconds.
    ///
    /// Pass `None` to disable timeout.
    #[inline]
    pub fn timeout<T>(mut self, timeout: T) -> ClientBuilder
    where T: Into<Option<Duration>>,
    {
        self.timeout = Timeout(timeout.into());
        self
    }

    fn with_inner<F>(mut self, func: F) -> ClientBuilder
    where
        F: FnOnce(async_impl::ClientBuilder) -> async_impl::ClientBuilder,
    {
        self.inner = func(self.inner);
        self
    }
}


impl Client {
    /// Constructs a new `Client`.
    ///
    /// # Panic
    ///
    /// This method panics if native TLS backend cannot be created or
    /// initialized. Use `Client::builder()` if you wish to handle the failure
    /// as an `Error` instead of panicking.
    #[inline]
    pub fn new() -> Client {
        ClientBuilder::new()
            .build()
            .expect("Client failed to initialize")
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
    pub fn execute(&self, request: Request) -> ::Result<Response> {
        self.inner.execute_request(request)
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
            //.field("gzip", &self.inner.gzip)
            //.field("redirect_policy", &self.inner.redirect_policy)
            //.field("referer", &self.inner.referer)
            .finish()
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .finish()
    }
}

#[derive(Clone)]
struct ClientHandle {
    timeout: Timeout,
    inner: Arc<InnerClientHandle>
}

type ThreadSender = mpsc::UnboundedSender<(async_impl::Request, oneshot::Sender<::Result<async_impl::Response>>)>;

struct InnerClientHandle {
    tx: Option<ThreadSender>,
    thread: Option<thread::JoinHandle<()>>
}

impl Drop for InnerClientHandle {
    fn drop(&mut self) {
        self.tx.take();
        self.thread.take().map(|h| h.join());
    }
}

impl ClientHandle {
    fn new(builder: ClientBuilder) -> ::Result<ClientHandle> {
        let timeout = builder.timeout;
        let builder = builder.inner;
        let (tx, rx) = mpsc::unbounded();
        let (spawn_tx, spawn_rx) = oneshot::channel::<::Result<()>>();
        let handle = try_!(thread::Builder::new().name("reqwest-internal-sync-runtime".into()).spawn(move || {
            use tokio::runtime::current_thread::Runtime;

            let built = (|| {
                let rt = try_!(Runtime::new());
                let client = builder.build()?;
                Ok((rt, client))
            })();

            let (mut rt, client) = match built {
                Ok((rt, c)) => {
                    if let Err(_) = spawn_tx.send(Ok(())) {
                        return;
                    }
                    (rt, c)
                },
                Err(e) => {
                    let _ = spawn_tx.send(Err(e));
                    return;
                }
            };

            let work = rx.for_each(move |(req, tx)| {
                let tx: oneshot::Sender<::Result<async_impl::Response>> = tx;
                let task = client.execute(req)
                    .then(move |x| tx.send(x).map_err(|_| ()));
                ::tokio::spawn(task);
                Ok(())
            });


            // work is Future<(), ()>, and our closure will never return Err
            rt.spawn(work)
                .run()
                .expect("runtime unexpected error");
        }));

        wait::timeout(spawn_rx, timeout.0).expect("runtime thread cancelled")?;

        let inner_handle = Arc::new(InnerClientHandle {
            tx: Some(tx),
            thread: Some(handle)
        });


        Ok(ClientHandle {
            timeout: timeout,
            inner: inner_handle,
        })
    }

    fn execute_request(&self, req: Request) -> ::Result<Response> {
        let (tx, rx) = oneshot::channel();
        let (req, body) = req.into_async();
        let url = req.url().clone();
        self.inner.tx
            .as_ref()
            .expect("core thread exited early")
            .unbounded_send((req, tx))
            .expect("core thread panicked");

        if let Some(body) = body {
            try_!(body.send(), &url);
        }

        let res = match wait::timeout(rx, self.timeout.0) {
            Ok(res) => res,
            Err(wait::Waited::TimedOut) => return Err(::error::timedout(Some(url))),
            Err(wait::Waited::Err(_canceled)) => {
                // The only possible reason there would be a Cancelled error
                // is if the thread running the Core panicked. We could return
                // an Err here, like a BrokenPipe, but the Client is not
                // recoverable. Additionally, the panic in the other thread
                // is not normal, and should likely be propagated.
                panic!("core thread panicked");
            }
        };
        res.map(|res| {
            response::new(res, self.timeout.0, KeepCoreThreadAlive(self.inner.clone()))
        })
    }
}

#[derive(Clone, Copy)]
struct Timeout(Option<Duration>);

impl Default for Timeout {
    #[inline]
    fn default() -> Timeout {
        // default mentioned in ClientBuilder::timeout() doc comment
        Timeout(Some(Duration::from_secs(30)))
    }
}

// pub(crate)

pub struct KeepCoreThreadAlive(Arc<InnerClientHandle>);
