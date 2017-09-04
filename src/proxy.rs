use std::fmt;
use std::sync::Arc;

use hyper::Uri;
use {into_url, IntoUrl, Url};

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
        let uri = ::into_url::to_uri(&try_!(url.into_url()));
        Ok(Proxy::new(Intercept::Http(uri)))
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
        let uri = ::into_url::to_uri(&try_!(url.into_url()));
        Ok(Proxy::new(Intercept::Https(uri)))
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
        let uri = ::into_url::to_uri(&try_!(url.into_url()));
        Ok(Proxy::new(Intercept::All(uri)))
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
    pub fn custom<F>(fun: F) -> Proxy
    where F: Fn(&Url) -> Option<Url> + Send + Sync + 'static {
        Proxy::new(Intercept::Custom(Custom(Arc::new(fun))))
    }

    /*
    pub fn unix<P: AsRef<Path>(path: P) -> Proxy {

    }
    */

    fn new(intercept: Intercept) -> Proxy {
        Proxy {
            intercept: intercept,
        }
    }

    fn proxies(&self, url: &Url) -> bool {
        match self.intercept {
            Intercept::All(..) => true,
            Intercept::Http(..) => url.scheme() == "http",
            Intercept::Https(..) => url.scheme() == "https",
            Intercept::Custom(ref fun) => (fun.0)(url).is_some(),
        }
    }


    fn intercept(&self, uri: &Uri) -> Option<Uri> {
        match self.intercept {
            Intercept::All(ref u) => Some(u.clone()),
            Intercept::Http(ref u) => {
                if uri.scheme() == Some("http") {
                    Some(u.clone())
                } else {
                    None
                }
            },
            Intercept::Https(ref u) => {
                if uri.scheme() == Some("https") {
                    Some(u.clone())
                } else {
                    None
                }
            },
            Intercept::Custom(ref fun) => {
                (fun.0)(&into_url::to_url(uri))
                    .map(|u| into_url::to_uri(&u))
            },
        }
    }
}

#[derive(Clone, Debug)]
enum Intercept {
    All(Uri),
    Http(Uri),
    Https(Uri),
    Custom(Custom),
}

#[derive(Clone)]
struct Custom(Arc<Fn(&Url) -> Option<Url> + Send + Sync + 'static>);

impl fmt::Debug for Custom {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("_")
    }
}

// pub(crate)

pub fn intercept(proxy: &Proxy, uri: &Uri) -> Option<Uri> {
    proxy.intercept(uri)
}

pub fn is_proxied(proxies: &[Proxy], uri: &Url) -> bool {
    proxies.iter().any(|p| p.proxies(uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    fn url(s: &str) -> Url {
        s.parse().unwrap()
    }

    #[test]
    fn test_http() {
        let target = "http://example.domain/";
        let p = Proxy::http(target).unwrap();

        let http = "http://hyper.rs";
        let other = "https://hyper.rs";

        assert!(p.proxies(&url(http)));
        assert_eq!(p.intercept(&uri(http)).unwrap(), target);
        assert!(!p.proxies(&url(other)));
        assert!(p.intercept(&uri(other)).is_none());
    }

    #[test]
    fn test_https() {
        let target = "http://example.domain/";
        let p = Proxy::https(target).unwrap();

        let http = "http://hyper.rs";
        let other = "https://hyper.rs";

        assert!(!p.proxies(&url(http)));
        assert!(p.intercept(&uri(http)).is_none());
        assert!(p.proxies(&url(other)));
        assert_eq!(p.intercept(&uri(other)).unwrap(), target);
    }

    #[test]
    fn test_all() {
        let target = "http://example.domain/";
        let p = Proxy::all(target).unwrap();

        let http = "http://hyper.rs";
        let https = "https://hyper.rs";
        let other = "x-youve-never-heard-of-me-mr-proxy://hyper.rs";

        assert!(p.proxies(&url(http)));
        assert!(p.proxies(&url(https)));
        assert!(p.proxies(&url(other)));

        assert_eq!(p.intercept(&uri(http)).unwrap(), target);
        assert_eq!(p.intercept(&uri(https)).unwrap(), target);
        assert_eq!(p.intercept(&uri(other)).unwrap(), target);
    }


    #[test]
    fn test_custom() {
        let target1 = "http://example.domain/";
        let target2 = "https://example.domain/";
        let p = Proxy::custom(move |url| {
            if url.host_str() == Some("hyper.rs") {
                target1.parse().ok()
            } else if url.scheme() == "http" {
                target2.parse().ok()
            } else {
                None
            }
        });

        let http = "http://seanmonstar.com";
        let https = "https://hyper.rs";
        let other = "x-youve-never-heard-of-me-mr-proxy://seanmonstar.com";

        assert!(p.proxies(&url(http)));
        assert!(p.proxies(&url(https)));
        assert!(!p.proxies(&url(other)));

        assert_eq!(p.intercept(&uri(http)).unwrap(), target2);
        assert_eq!(p.intercept(&uri(https)).unwrap(), target1);
        assert!(p.intercept(&uri(other)).is_none());
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
