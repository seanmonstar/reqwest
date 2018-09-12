use std::fmt;
use std::sync::Arc;
use std::net::{SocketAddr, ToSocketAddrs};

use hyper::client::connect::Destination;
use url::percent_encoding::percent_decode;
use {IntoUrl, Url};


// The kinds of authentication supported to a proxy
#[derive(Clone, Debug)]
pub(crate) enum ProxyAuth {
    Basic {
        username: String,
        password: String,
    }
}

// The various proxy schemes we understand
#[derive(Clone, Debug)]
pub(crate) enum ProxySchemeKind {
    Http,
    Https,
    Socks5,
    Socks5h,
}

/// A particular scheme used for proxying requests.
/// 
/// For example, HTTP vs SOCKS5
#[derive(Clone, Debug)]
pub struct ProxyScheme {
    pub(crate) kind: ProxySchemeKind,
    pub(crate) uri: Option<::hyper::Uri>,
    pub(crate) socket_addrs: Option<Vec<SocketAddr>>,
    pub(crate) auth: Option<ProxyAuth>,
}

impl ProxyScheme {
    /// Proxy traffic via the specified URL over HTTP
    pub fn http<T: IntoUrl>(url: T) -> ::Result<Self> {
        Ok(ProxyScheme {
            kind: ProxySchemeKind::Http,
            uri: Some(::into_url::to_uri(&url.into_url()?)),
            socket_addrs: None,
            auth: None,
        })
    }

    /// Proxy traffic via the specified URL over HTTPS
    pub fn https<T: IntoUrl>(url: T) -> ::Result<Self> {
        Ok(ProxyScheme {
            kind: ProxySchemeKind::Https,
            uri: Some(::into_url::to_uri(&url.into_url()?)),
            socket_addrs: None,
            auth: None,
        })
    }

    /// Proxy traffic via the specified socket address over SOCKS5
    pub fn socks5<T: ToSocketAddrs>(socket_addr: T) -> ::Result<Self> {
        Ok(ProxyScheme {
            kind: ProxySchemeKind::Socks5,
            uri: None,
            socket_addrs: Some(try_!(socket_addr.to_socket_addrs()).collect()),
            auth: None,
        })
    }

    /// Proxy traffic via the specified socket address over SOCKS5H
    /// This differs from SOCKS5 in that DNS resolution is also performed via the proxy.
    pub fn socks5h<T: ToSocketAddrs>(socket_addr: T) -> ::Result<Self> {
        Ok(ProxyScheme {
            kind: ProxySchemeKind::Socks5h,
            uri: None,
            socket_addrs: Some(try_!(socket_addr.to_socket_addrs()).collect()),
            auth: None,
        })
    }

    /// Use a username and password when connecting to the proxy server
    pub fn with_basic_auth<T: Into<String>, U: Into<String>>(mut self, username: T, password: U) -> Self {
        self.auth = Some(ProxyAuth::Basic {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    /// Convert a URL into a proxy scheme
    /// 
    /// Supported schemes: HTTP, HTTPS, SOCKS5, SOCKS5H
    pub fn parse(url: Url) -> ::Result<Self> {
        // Resolve URL to a host and port
        let host_and_port = try_!(url.with_default_port(|url| match url.scheme() {
            "socks5" | "socks5h" => Ok(1080),
            _ => Err(())
        }));

        let mut scheme = match url.scheme() {
            "http" => Self::http(url.clone())?,
            "https" => Self::https(url.clone())?,
            "socks5" => Self::socks5(host_and_port)?,
            "socks5h" => Self::socks5h(host_and_port)?,
            _ => return Err(::error::unknown_proxy_scheme())
        };

        if let Some(pwd) = url.password() {
            let decoded_username = percent_decode(url.username().as_bytes()).decode_utf8_lossy();
            let decoded_password = percent_decode(pwd.as_bytes()).decode_utf8_lossy();
            scheme = scheme.with_basic_auth(decoded_username, decoded_password);
        }

        Ok(scheme)
    }
}

/// Trait used for converting into a proxy scheme. This trait supports
/// parsing from a URL-like type, whilst also supporting proxy schemes
/// built directly using the factory methods.
pub trait IntoProxyScheme {
    /// Perform the conversion
    fn into_proxy_scheme(self) -> ::Result<ProxyScheme>;
}

impl<T: IntoUrl> IntoProxyScheme for T {
    fn into_proxy_scheme(self) -> ::Result<ProxyScheme> {
        ProxyScheme::parse(self.into_url()?)
    }
}

impl IntoProxyScheme for ProxyScheme {
    fn into_proxy_scheme(self) -> ::Result<ProxyScheme> {
        Ok(self)
    }
}

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
    pub fn http<U: IntoProxyScheme>(proxy_scheme: U) -> ::Result<Proxy> {
        Ok(Proxy::new(Intercept::Http(
            proxy_scheme.into_proxy_scheme()?
        )))
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
    pub fn https<U: IntoProxyScheme>(proxy_scheme: U) -> ::Result<Proxy> {
        Ok(Proxy::new(Intercept::Https(
            proxy_scheme.into_proxy_scheme()?
        )))
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
    pub fn all<U: IntoProxyScheme>(proxy_scheme: U) -> ::Result<Proxy> {
        Ok(Proxy::new(Intercept::All(
            proxy_scheme.into_proxy_scheme()?
        )))
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
    pub fn custom<F, U: IntoProxyScheme>(fun: F) -> Proxy
    where F: Fn(&Url) -> Option<U> + Send + Sync + 'static {
        Proxy::new(Intercept::Custom(Custom(Arc::new(move |url| {
            fun(url).map(IntoProxyScheme::into_proxy_scheme)
        }))))
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

    pub(crate) fn intercept<D: Dst>(&self, uri: &D) -> Option<::Result<ProxyScheme>> {
        match self.intercept {
            Intercept::All(ref u) => Some(Ok(u.clone())),
            Intercept::Http(ref u) => {
                if uri.scheme() == "http" {
                    Some(Ok(u.clone()))
                } else {
                    None
                }
            },
            Intercept::Https(ref u) => {
                if uri.scheme() == "https" {
                    Some(Ok(u.clone()))
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
            },
        }
    }
}

#[derive(Clone, Debug)]
enum Intercept {
    All(ProxyScheme),
    Http(ProxyScheme),
    Https(ProxyScheme),
    Custom(Custom),
}

#[derive(Clone)]
struct Custom(Arc<Fn(&Url) -> Option<::Result<ProxyScheme>> + Send + Sync + 'static>);

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

        assert_eq!(p.intercept(&url(http)).unwrap().unwrap().uri.unwrap(), target);
        assert!(p.intercept(&url(other)).is_none());
    }

    #[test]
    fn test_https() {
        let target = "http://example.domain/";
        let p = Proxy::https(target).unwrap();

        let http = "http://hyper.rs";
        let other = "https://hyper.rs";

        assert!(p.intercept(&url(http)).is_none());
        assert_eq!(p.intercept(&url(other)).unwrap().unwrap().uri.unwrap(), target);
    }

    #[test]
    fn test_all() {
        let target = "http://example.domain/";
        let p = Proxy::all(target).unwrap();

        let http = "http://hyper.rs";
        let https = "https://hyper.rs";
        let other = "x-youve-never-heard-of-me-mr-proxy://hyper.rs";

        assert_eq!(p.intercept(&url(http)).unwrap().unwrap().uri.unwrap(), target);
        assert_eq!(p.intercept(&url(https)).unwrap().unwrap().uri.unwrap(), target);
        assert_eq!(p.intercept(&url(other)).unwrap().unwrap().uri.unwrap(), target);
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
                None::<Url>
            }
        });

        let http = "http://seanmonstar.com";
        let https = "https://hyper.rs";
        let other = "x-youve-never-heard-of-me-mr-proxy://seanmonstar.com";

        assert_eq!(p.intercept(&url(http)).unwrap().unwrap().uri.unwrap(), target2);
        assert_eq!(p.intercept(&url(https)).unwrap().unwrap().uri.unwrap(), target1);
        assert!(p.intercept(&url(other)).is_none());
    }

}
