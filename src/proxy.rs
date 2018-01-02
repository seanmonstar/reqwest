use std::any::Any;
use hyper::Uri;
use hyper::header::{Scheme};
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
/// use reqwest::header::Basic;
///
/// let mut proxy = reqwest::Proxy::http("https://secure.example")?;
/// proxy.set_authorization(Basic {
///     username: "John Doe".into(),
///     password: Some("Agent1234".into()),
/// });
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
#[derive(Clone, Debug)]
pub struct Proxy {
    pub(crate) inner: HyperProxy,
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
    ///     .proxy(reqwest::Proxy::custom(|url| url.host() == Some("hyper.rs"),
    ///                                   "http://proxy.custom")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    pub fn custom<F, U: IntoUrl>(fun: F, url: U) -> ::Result<Proxy>
    where F: Fn(&Uri) -> bool + 'static + Send + Sync {
        Proxy::new(fun, url)
    }

    /// Set proxy authorization
    pub fn set_authorization<S: Scheme + Any>(&mut self, scheme: S) {
        self.inner.set_authorization(scheme);
    }

    /*
    pub fn unix<P: AsRef<Path>(path: P) -> Proxy {

    }
    */

    fn new<U: IntoUrl, I: Into<Intercept>>(intercept: I, url: U) -> ::Result<Proxy> {
        let uri = ::into_url::to_uri(&try_!(url.into_url()));
        Ok(Proxy { inner: HyperProxy::new(intercept, uri) })
    }
}
