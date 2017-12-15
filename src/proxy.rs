use hyper::Uri;
use {IntoUrl};
use hyper_proxy::Intercept;
use hyper_proxy::Proxy as HyperProxy;

/// Configuration of a proxy that a `Client` should pass requests to.
///
/// A `Proxy` has a couple pieces to it:
///
/// - a URL of how to talk to the proxy
/// - rules on what `Client` requests should be directed to the proxy
///
/// For instance, let's look at `Proxy::http`:
///
/// ```rust
/// # fn run() -> Result<(), Box<::std::error::Error>> {
/// let mut proxy = reqwest::Proxy::http("https://secure.example")?;
/// // proxy.set_authorization(Basic {
/// //     username: "John Doe".into(),
/// //     password: Some("Agent1234".into()),
/// // });
/// # Ok(())
/// # }
/// ```
///
/// This proxy will intercept all HTTP requests, and make use of the proxy
/// at `https://secure.example`. A request to `http://hyper.rs` will talk
/// to your proxy. A request to `https://hyper.rs` will not.
///
/// Multiple `Proxy` rules can be configured for a `Client`. The `Client` will
/// check each `Proxy` in the order it was added. This could mean that a
/// `Proxy` added first with eager intercept rules, such as `Proxy::all`,
/// would prevent a `Proxy` later in the list from ever working, so take care.
#[derive(Debug)]
pub struct Proxy {
    pub(crate) inner: HyperProxy<()>,
}

impl Proxy {
    /// Proxy all HTTP traffic to the passed URL.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = reqwest::Client::builder()
    ///     .proxy(reqwest::Proxy::http("https://my.prox")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn http<U: IntoUrl>(url: U) -> ::Result<Proxy> {
        Proxy::new(Intercept::Http, url)
    }

    /// Proxy all HTTPS traffic to the passed URL.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = reqwest::Client::builder()
    ///     .proxy(reqwest::Proxy::https("https://example.prox:4545")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn https<U: IntoUrl>(url: U) -> ::Result<Proxy> {
        Proxy::new(Intercept::Https, url)
    }

    /// Proxy **all** traffic to the passed URL.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = reqwest::Client::builder()
    ///     .proxy(reqwest::Proxy::all("http://pro.xy")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn all<U: IntoUrl>(url: U) -> ::Result<Proxy> {
        Proxy::new(Intercept::All, url)
    }

    /// Provide a custom function to determine what traffix to proxy to where.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let target = reqwest::Url::parse("https://my.prox")?;
    /// let client = reqwest::Client::builder()
    ///     .proxy(reqwest::Proxy::custom(move |url| {
    ///         if url.host_str() == Some("hyper.rs") {
    ///             Some(target.clone())
    ///         } else {
    ///             None
    ///         }
    ///     }))
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    pub fn custom<F, U: IntoUrl>(fun: F, url: U) -> ::Result<Proxy>
    where F: Fn(&Uri) -> bool + 'static + Send + Sync {
        Proxy::new(Intercept::Custom(Box::new(fun)), url)
    }

    /*
    pub fn unix<P: AsRef<Path>(path: P) -> Proxy {

    }
    */

    /// Get a new empty proxy which will never intercept Uris
    pub(crate) fn empty() -> Proxy {
        Proxy { inner: HyperProxy::unsecured((), Intercept::None, Uri::default()) }
    }

    fn new<U: IntoUrl>(intercept: Intercept, url: U) -> ::Result<Proxy> {
        let uri = ::into_url::to_uri(&try_!(url.into_url()));
        Ok(Proxy { inner: try_!(HyperProxy::new((), intercept, uri)) })
    }
}
