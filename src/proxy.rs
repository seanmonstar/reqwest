use hyper::Uri;
use {IntoUrl};

/// Configuration of a proxy that a `Client` should pass requests to.
///
/// A `Proxy` has a couple pieces to it:
///
/// - a URL of how to talk to the proxy
/// - rules on what `Client` requests should be directed to the proxy
///
/// For instance, let's look at `Proxy::http`:
///
/// ```
/// # extern crate reqwest;
/// # fn run() -> Result<(), Box<::std::error::Error>> {
/// let proxy = reqwest::Proxy::http("https://secure.example")?;
/// # Ok(())
/// # }
/// # fn main() {}
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
    intercept: Intercept,
    uri: Uri,
}

impl Proxy {
    /// Proxy all HTTP traffic to the passed URL.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = reqwest::Client::builder()?
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
    /// let client = reqwest::Client::builder()?
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
    /// let client = reqwest::Client::builder()?
    ///     .proxy(reqwest::Proxy::all("http://pro.xy")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn all<U: IntoUrl>(url: U) -> ::Result<Proxy> {
        Proxy::new(Intercept::All, url)
    }

    /*
    pub fn unix<P: AsRef<Path>(path: P) -> Proxy {

    }
    */

    fn new<U: IntoUrl>(intercept: Intercept, url: U) -> ::Result<Proxy> {
        let uri = ::into_url::to_uri(&try_!(url.into_url()));
        Ok(Proxy {
            intercept: intercept,
            uri: uri,
        })
    }

    fn proxies(&self, uri: &Uri) -> bool {
        match self.intercept {
            Intercept::All => true,
            Intercept::Http => uri.scheme() == Some("http"),
            Intercept::Https => uri.scheme() == Some("https"),
        }
    }
}

#[derive(Clone, Debug)]
enum Intercept {
    All,
    Http,
    Https,
}

// pub(crate)

pub fn proxies(proxy: &Proxy, uri: &Uri) -> Option<Uri> {
    if proxy.proxies(uri) {
        Some(proxy.uri.clone())
    } else {
        None
    }
}

pub fn is_proxied(proxies: &[Proxy], uri: &Uri) -> bool {
    proxies.iter().any(|p| p.proxies(uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http() {
        let p = Proxy::http("http://example.domain").unwrap();

        let http = "http://hyper.rs".parse().unwrap();
        let other = "https://hyper.rs".parse().unwrap();

        assert!(p.proxies(&http));
        assert!(!p.proxies(&other));
    }

    #[test]
    fn test_https() {
        let p = Proxy::https("http://example.domain").unwrap();

        let http = "http://hyper.rs".parse().unwrap();
        let other = "https://hyper.rs".parse().unwrap();

        assert!(!p.proxies(&http));
        assert!(p.proxies(&other));
    }

    #[test]
    fn test_all() {
        let p = Proxy::all("http://example.domain").unwrap();

        let http = "http://hyper.rs".parse().unwrap();
        let https = "https://hyper.rs".parse().unwrap();
        let other = "x-youve-never-heard-of-me-mr-proxy://hyper.rs".parse().unwrap();

        assert!(p.proxies(&http));
        assert!(p.proxies(&https));
        assert!(p.proxies(&other));
    }

    #[test]
    fn test_is_proxied() {
        let proxies = vec![
            Proxy::http("http://example.domain").unwrap(),
            Proxy::https("http://other.domain").unwrap(),
        ];

        let http = "http://hyper.rs".parse().unwrap();
        let https = "https://hyper.rs".parse().unwrap();
        let other = "x-other://hyper.rs".parse().unwrap();

        assert!(is_proxied(&proxies, &http));
        assert!(is_proxied(&proxies, &https));
        assert!(!is_proxied(&proxies, &other));
    }

}
