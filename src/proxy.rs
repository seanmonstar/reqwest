use std::fmt;
use std::sync::Arc;
#[cfg(feature = "socks")]
use std::net::{SocketAddr, ToSocketAddrs};

use http::{header::HeaderValue, Uri};
use hyper::client::connect::Destination;
use url::percent_encoding::percent_decode;
use {IntoUrl, Url};
use std::collections::HashMap;
use std::env;
#[cfg(target_os = "windows")]
use std::error::Error;
#[cfg(target_os = "windows")]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(target_os = "windows")]
use winreg::RegKey;

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

/// A particular scheme used for proxying requests.
///
/// For example, HTTP vs SOCKS5
#[derive(Clone, Debug)]
pub enum ProxyScheme {
    Http {
        auth: Option<HeaderValue>,
        uri: ::hyper::Uri,
    },
    #[cfg(feature = "socks")]
    Socks5 {
        addr: SocketAddr,
        auth: Option<(String, String)>,
        remote_dns: bool,
    },
}

/// Trait used for converting into a proxy scheme. This trait supports
/// parsing from a URL-like type, whilst also supporting proxy schemes
/// built directly using the factory methods.
pub trait IntoProxyScheme {
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
        Proxy::new(Intercept::Custom(Custom {
            auth: None,
            func: Arc::new(move |url| {
                fun(url).map(IntoProxyScheme::into_proxy_scheme)
            }),
        }))
    }

    /*
    pub fn unix<P: AsRef<Path>(path: P) -> Proxy {

    }
    */

    fn new(intercept: Intercept) -> Proxy {
        Proxy {
            intercept,
        }
    }

    /// Set the `Proxy-Authorization` header using Basic auth.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let proxy = reqwest::Proxy::https("http://localhost:1234")?
    ///     .basic_auth("Aladdin", "open sesame");
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn basic_auth(mut self, username: &str, password: &str) -> Proxy {
        self.intercept.set_basic_auth(username, password);
        self
    }

    pub(crate) fn maybe_has_http_auth(&self) -> bool {
        match self.intercept {
            Intercept::All(ProxyScheme::Http { auth: Some(..), .. }) |
            Intercept::Http(ProxyScheme::Http { auth: Some(..), .. }) |
            // Custom *may* match 'http', so assume so.
            Intercept::Custom(_) => true,
            _ => false,
        }
    }

    pub(crate) fn http_basic_auth<D: Dst>(&self, uri: &D) -> Option<HeaderValue> {
        match self.intercept {
            Intercept::All(ProxyScheme::Http { ref auth, .. }) |
            Intercept::Http(ProxyScheme::Http { ref auth, .. }) => auth.clone(),
            Intercept::Custom(ref custom) => {
                custom.call(uri).and_then(|scheme| match scheme {
                    ProxyScheme::Http { auth, .. } => auth,
                    #[cfg(feature = "socks")]
                    _ => None,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn intercept<D: Dst>(&self, uri: &D) -> Option<ProxyScheme> {
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
            Intercept::Custom(ref custom) => custom.call(uri),
        }
    }

    pub(crate) fn is_match<D: Dst>(&self, uri: &D) -> bool {
        match self.intercept {
            Intercept::All(_) => true,
            Intercept::Http(_) => {
                uri.scheme() == "http"
            },
            Intercept::Https(_) => {
                uri.scheme() == "https"
            },
            Intercept::Custom(ref custom) => custom.call(uri).is_some(),
        }
    }
}

impl ProxyScheme {
    // To start conservative, keep builders private for now.

    /// Proxy traffic via the specified URL over HTTP
    fn http<T: IntoUrl>(url: T) -> ::Result<Self> {
        Ok(ProxyScheme::Http {
            auth: None,
            uri: ::into_url::expect_uri(&url.into_url()?),
        })
    }

    /// Proxy traffic via the specified socket address over SOCKS5
    ///
    /// # Note
    ///
    /// Current SOCKS5 support is provided via blocking IO.
    #[cfg(feature = "socks")]
    fn socks5(addr: SocketAddr) -> ::Result<Self> {
        Ok(ProxyScheme::Socks5 {
            addr,
            auth: None,
            remote_dns: false,
        })
    }

    /// Proxy traffic via the specified socket address over SOCKS5H
    ///
    /// This differs from SOCKS5 in that DNS resolution is also performed via the proxy.
    ///
    /// # Note
    ///
    /// Current SOCKS5 support is provided via blocking IO.
    #[cfg(feature = "socks")]
    fn socks5h(addr: SocketAddr) -> ::Result<Self> {
        Ok(ProxyScheme::Socks5 {
            addr,
            auth: None,
            remote_dns: true,
        })
    }

    /// Use a username and password when connecting to the proxy server
    fn with_basic_auth<T: Into<String>, U: Into<String>>(mut self, username: T, password: U) -> Self {
        self.set_basic_auth(username, password);
        self
    }

    fn set_basic_auth<T: Into<String>, U: Into<String>>(&mut self, username: T, password: U) {
        match *self {
            ProxyScheme::Http { ref mut auth, .. } => {
                let header = encode_basic_auth(&username.into(), &password.into());
                *auth = Some(header);
            },
            #[cfg(feature = "socks")]
            ProxyScheme::Socks5 { ref mut auth, .. } => {
                *auth = Some((username.into(), password.into()));
            }
        }
    }

    /// Convert a URL into a proxy scheme
    ///
    /// Supported schemes: HTTP, HTTPS, (SOCKS5, SOCKS5H if `socks` feature is enabled).
    // Private for now...
    fn parse(url: Url) -> ::Result<Self> {
        // Resolve URL to a host and port
        #[cfg(feature = "socks")]
        let to_addr = || {
            let host_and_port = try_!(url.with_default_port(|url| match url.scheme() {
                "socks5" | "socks5h" => Ok(1080),
                _ => Err(())
            }));
            let mut addr = try_!(host_and_port.to_socket_addrs());
            addr
                .next()
                .ok_or_else(::error::unknown_proxy_scheme)
        };

        let mut scheme = match url.scheme() {
            "http" | "https" => Self::http(url.clone())?,
            #[cfg(feature = "socks")]
            "socks5" => Self::socks5(to_addr()?)?,
            #[cfg(feature = "socks")]
            "socks5h" => Self::socks5h(to_addr()?)?,
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



#[derive(Clone, Debug)]
enum Intercept {
    All(ProxyScheme),
    Http(ProxyScheme),
    Https(ProxyScheme),
    Custom(Custom),
}

impl Intercept {
    fn set_basic_auth(&mut self, username: &str, password: &str) {
        match self {
            Intercept::All(ref mut s) |
            Intercept::Http(ref mut s) |
            Intercept::Https(ref mut s) => s.set_basic_auth(username, password),
            Intercept::Custom(ref mut custom) => {
                let header = encode_basic_auth(username, password);
                custom.auth = Some(header);
            }
        }
    }
}

#[derive(Clone)]
struct Custom {
    // This auth only applies if the returned ProxyScheme doesn't have an auth...
    auth: Option<HeaderValue>,
    func: Arc<dyn Fn(&Url) -> Option<::Result<ProxyScheme>> + Send + Sync + 'static>,
}

impl Custom {
    fn call<D: Dst>(&self, uri: &D) -> Option<ProxyScheme> {
        let url = format!(
            "{}://{}{}{}",
            uri.scheme(),
            uri.host(),
            uri.port().map(|_| ":").unwrap_or(""),
            uri.port().map(|p| p.to_string()).unwrap_or(String::new())
        )
            .parse()
            .expect("should be valid Url");

        (self.func)(&url)
            .and_then(|result| result.ok())
            .map(|scheme| match scheme {
                ProxyScheme::Http { auth, uri } => {
                    if auth.is_some() {
                        ProxyScheme::Http { auth, uri }
                    } else {
                        ProxyScheme::Http {
                            auth: self.auth.clone(),
                            uri,
                        }
                    }
                },
                #[cfg(feature = "socks")]
                socks => socks,
            })
    }
}

impl fmt::Debug for Custom {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("_")
    }
}

pub(crate) fn encode_basic_auth(username: &str, password: &str) -> HeaderValue {
    let val = format!("{}:{}", username, password);
    let mut header = format!("Basic {}", base64::encode(&val))
        .parse::<HeaderValue>()
        .expect("base64 is always valid HeaderValue");
    header.set_sensitive(true);
    header
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

#[doc(hidden)]
impl Dst for Uri {
    fn scheme(&self) -> &str {
        self.scheme_part()
            .expect("Uri should have a scheme")
            .as_str()
    }

    fn host(&self) -> &str {
        Uri::host(self)
            .expect("<Uri as Dst>::host should have a str")
    }

    fn port(&self) -> Option<u16> {
        self.port_part().map(|p| p.as_u16())
    }
}

/// Get system proxies information.
///
/// It can only support Linux, Unix like, and windows system.  Note that it will always
/// return a HashMap, even if something runs into error when find registry information in
/// Windows system.  Note that invalid proxy url in the system setting will be ignored.
///
/// Returns:
///     System proxies information as a hashmap like
///     {"http": Url::parse("http://127.0.0.1:80"), "https": Url::parse("https://127.0.0.1:80")}
pub fn get_proxies() -> HashMap<String, Url> {
    let proxies: HashMap<String, Url> = get_from_environment();

    // TODO: move the following #[cfg] to `if expression` when attributes on `if` expressions allowed
    #[cfg(target_os = "windows")]
    {
        if proxies.is_empty() {
            // don't care errors if can't get proxies from registry, just return an empty HashMap.
            return get_from_registry();
        }
    }
    proxies
}

fn insert_proxy(proxies: &mut HashMap<String, Url>, schema: String, addr: String)
{
    if let Ok(valid_addr) = Url::parse(&addr) {
        proxies.insert(schema, valid_addr);
    }
}

fn get_from_environment() -> HashMap<String, Url> {
    let mut proxies: HashMap<String, Url> = HashMap::new();

    const PROXY_KEY_ENDS: &str = "_proxy";

    for (key, value) in env::vars() {
        let key: String = key.to_lowercase();
        if key.ends_with(PROXY_KEY_ENDS) {
            let end_indx = key.len() - PROXY_KEY_ENDS.len();
            let schema = &key[..end_indx];
            insert_proxy(&mut proxies, String::from(schema), String::from(value));
        }
    }
    proxies
}


#[cfg(target_os = "windows")]
fn get_from_registry_impl() -> Result<HashMap<String, Url>, Box<dyn Error>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_setting: RegKey =
        hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")?;
    // ensure the proxy is enable, if the value doesn't exist, an error will returned.
    let proxy_enable: u32 = internet_setting.get_value("ProxyEnable")?;
    let proxy_server: String = internet_setting.get_value("ProxyServer")?;

    if proxy_enable == 0 {
        return Ok(HashMap::new());
    }

    let mut proxies: HashMap<String, Url> = HashMap::new();
    if proxy_server.contains("=") {
        // per-protocol settings.
        for p in proxy_server.split(";") {
            let protocol_parts: Vec<&str> = p.split("=").collect();
            match protocol_parts.as_slice() {
                [protocol, address] => {
                    insert_proxy(&mut proxies, String::from(*protocol), String::from(*address));
                }
                _ => {
                    // Contains invalid protocol setting, just break the loop
                    // And make proxies to be empty.
                    proxies.clear();
                    break;
                }
            }
        }
    } else {
        // Use one setting for all protocols.
        if proxy_server.starts_with("http:") {
            insert_proxy(&mut proxies, String::from("http"), proxy_server);
        } else {
            insert_proxy(&mut proxies, String::from("http"), format!("http://{}", proxy_server));
            insert_proxy(&mut proxies, String::from("https"), format!("https://{}", proxy_server));
            insert_proxy(&mut proxies, String::from("ftp"), format!("https://{}", proxy_server));
        }
    }
    Ok(proxies)
}

#[cfg(target_os = "windows")]
fn get_from_registry() -> HashMap<String, Url> {
    get_from_registry_impl().unwrap_or(HashMap::new())
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


    fn intercepted_uri(p: &Proxy, s: &str) -> Uri {
        match p.intercept(&url(s)).unwrap() {
            ProxyScheme::Http { uri, .. } => uri,
            #[cfg(feature = "socks")]
            _ => panic!("intercepted as socks"),
        }
    }

    #[test]
    fn test_http() {
        let target = "http://example.domain/";
        let p = Proxy::http(target).unwrap();

        let http = "http://hyper.rs";
        let other = "https://hyper.rs";

        assert_eq!(intercepted_uri(&p, http), target);
        assert!(p.intercept(&url(other)).is_none());
    }

    #[test]
    fn test_https() {
        let target = "http://example.domain/";
        let p = Proxy::https(target).unwrap();

        let http = "http://hyper.rs";
        let other = "https://hyper.rs";

        assert!(p.intercept(&url(http)).is_none());
        assert_eq!(intercepted_uri(&p, other), target);
    }

    #[test]
    fn test_all() {
        let target = "http://example.domain/";
        let p = Proxy::all(target).unwrap();

        let http = "http://hyper.rs";
        let https = "https://hyper.rs";
        let other = "x-youve-never-heard-of-me-mr-proxy://hyper.rs";

        assert_eq!(intercepted_uri(&p, http), target);
        assert_eq!(intercepted_uri(&p, https), target);
        assert_eq!(intercepted_uri(&p, other), target);
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

        assert_eq!(intercepted_uri(&p, http), target2);
        assert_eq!(intercepted_uri(&p, https), target1);
        assert!(p.intercept(&url(other)).is_none());
    }

    #[test]
    fn test_get_proxies() {
        // save system setting first.
        let system_proxy = env::var("http_proxy");

        // remove proxy.
        env::remove_var("http_proxy");
        assert_eq!(get_proxies().contains_key("http"), false);

        // the system proxy setting url is invalid.
        env::set_var("http_proxy", "123465");
        assert_eq!(get_proxies().contains_key("http"), false);

        // set valid proxy
        env::set_var("http_proxy", "http://127.0.0.1/");
        let proxies = get_proxies();
        let http_target = proxies.get("http").unwrap().as_str();

        assert_eq!(http_target, "http://127.0.0.1/");
        // reset user setting.
        match system_proxy {
            Err(_) => env::remove_var("http_proxy"),
            Ok(proxy) => env::set_var("http_proxy", proxy)
        }
    }
}
