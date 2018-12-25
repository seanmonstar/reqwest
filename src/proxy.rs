use std::fmt;
use std::sync::Arc;

use hyper::client::connect::Destination;
use crate::{into_url, IntoUrl, Url};

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
/// let proxy = reqwest::Proxy::http("https://secure.example")?;
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
    pub fn http<U: IntoUrl>(url: U) -> crate::Result<Proxy> {
        let uri = crate::into_url::to_uri(&url.into_url()?);
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
    pub fn https<U: IntoUrl>(url: U) -> crate::Result<Proxy> {
        let uri = crate::into_url::to_uri(&url.into_url()?);
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
    pub fn all<U: IntoUrl>(url: U) -> crate::Result<Proxy> {
        let uri = crate::into_url::to_uri(&url.into_url()?);
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

    pub(crate) fn intercept<D: Dst>(&self, uri: &D) -> Option<::hyper::Uri> {
        match self.intercept {
            Intercept::All(ref u) => Some(u.clone()),
            Intercept::Http(ref u) => {
                if uri.scheme() == "http" {
                    Some(u.clone())
                } else {
                    None
                }
            },
            Intercept::Https(ref u) => {
                if uri.scheme() == "https" {
                    Some(u.clone())
                } else {
                    None
                }
            },
            Intercept::Custom(ref fun) => {
                (fun.0)(
                    &format!(
                        "{}://{}{}{}",
                        uri.scheme(),
                        uri.host(),
                        uri.port().map(|_| ":").unwrap_or(""),
                        uri.port().map(|p| p.to_string()).unwrap_or(String::new())
                    )
                        .parse()
                        .expect("should be valid Url")
                )
                    .map(|u| into_url::to_uri(&u) )
            },
        }
    }
}

#[derive(Clone, Debug)]
enum Intercept {
    All(::hyper::Uri),
    Http(::hyper::Uri),
    Https(::hyper::Uri),
    Custom(Custom),
}

#[derive(Clone)]
struct Custom(Arc<Fn(&Url) -> Option<Url> + Send + Sync + 'static>);

impl fmt::Debug for Custom {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("_")
    }
}

/// A helper trait to allow testing `Proxy::intercept` without having to
/// construct `hyper::client::connect::Destination`s.
pub(crate) trait Dst {
    fn scheme(&self) -> &str;
    fn host(&self) -> &str;
    fn port(&self) -> Option<u16>;
}

#[doc(hidden)]
impl Dst for Destination {
    fn scheme(&self) -> &str {
        Destination::scheme(self)
    }

    fn host(&self) -> &str {
        Destination::host(self)
    }

    fn port(&self) -> Option<u16> {
        Destination::port(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Dst for Url {
        fn scheme(&self) -> &str {
            Url::scheme(self)
        }

        fn host(&self) -> &str {
            Url::host_str(self)
                .expect("<Url as Dst>::host should have a str")
        }

        fn port(&self) -> Option<u16> {
            Url::port(self)
        }
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

        assert_eq!(p.intercept(&url(http)).unwrap(), target);
        assert!(p.intercept(&url(other)).is_none());
    }

    #[test]
    fn test_https() {
        let target = "http://example.domain/";
        let p = Proxy::https(target).unwrap();

        let http = "http://hyper.rs";
        let other = "https://hyper.rs";

        assert!(p.intercept(&url(http)).is_none());
        assert_eq!(p.intercept(&url(other)).unwrap(), target);
    }

    #[test]
    fn test_all() {
        let target = "http://example.domain/";
        let p = Proxy::all(target).unwrap();

        let http = "http://hyper.rs";
        let https = "https://hyper.rs";
        let other = "x-youve-never-heard-of-me-mr-proxy://hyper.rs";

        assert_eq!(p.intercept(&url(http)).unwrap(), target);
        assert_eq!(p.intercept(&url(https)).unwrap(), target);
        assert_eq!(p.intercept(&url(other)).unwrap(), target);
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

        assert_eq!(p.intercept(&url(http)).unwrap(), target2);
        assert_eq!(p.intercept(&url(https)).unwrap(), target1);
        assert!(p.intercept(&url(other)).is_none());
    }

}
