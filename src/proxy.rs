use std::fmt;
#[cfg(feature = "socks")]
use std::net::SocketAddr;
use std::sync::Arc;

use crate::{IntoUrl, Url};
use http::{header::HeaderValue, Uri};
use percent_encoding::percent_decode;
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
/// # fn run() -> Result<(), Box<std::error::Error>> {
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
///
/// By enabling the `"socks"` feature it is possible to use a socks proxy:
/// ```rust
/// # fn run() -> Result<(), Box<std::error::Error>> {
/// let proxy = reqwest::Proxy::http("socks5://192.168.1.1:9000")?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
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
        host: http::uri::Authority,
    },
    Https {
        auth: Option<HeaderValue>,
        host: http::uri::Authority,
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
    fn into_proxy_scheme(self) -> crate::Result<ProxyScheme>;
}

impl<T: IntoUrl> IntoProxyScheme for T {
    fn into_proxy_scheme(self) -> crate::Result<ProxyScheme> {
        ProxyScheme::parse(self.into_url()?)
    }
}

impl IntoProxyScheme for ProxyScheme {
    fn into_proxy_scheme(self) -> crate::Result<ProxyScheme> {
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
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let client = reqwest::Client::builder()
    ///     .proxy(reqwest::Proxy::http("https://my.prox")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn http<U: IntoProxyScheme>(proxy_scheme: U) -> crate::Result<Proxy> {
        Ok(Proxy::new(Intercept::Http(
            proxy_scheme.into_proxy_scheme()?,
        )))
    }

    /// Proxy all HTTPS traffic to the passed URL.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let client = reqwest::Client::builder()
    ///     .proxy(reqwest::Proxy::https("https://example.prox:4545")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn https<U: IntoProxyScheme>(proxy_scheme: U) -> crate::Result<Proxy> {
        Ok(Proxy::new(Intercept::Https(
            proxy_scheme.into_proxy_scheme()?,
        )))
    }

    /// Proxy **all** traffic to the passed URL.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let client = reqwest::Client::builder()
    ///     .proxy(reqwest::Proxy::all("http://pro.xy")?)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// # fn main() {}
    /// ```
    pub fn all<U: IntoProxyScheme>(proxy_scheme: U) -> crate::Result<Proxy> {
        Ok(Proxy::new(Intercept::All(
            proxy_scheme.into_proxy_scheme()?,
        )))
    }

    /// Provide a custom function to determine what traffix to proxy to where.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
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
    where
        F: Fn(&Url) -> Option<U> + Send + Sync + 'static,
    {
        Proxy::new(Intercept::Custom(Custom {
            auth: None,
            func: Arc::new(move |url| fun(url).map(IntoProxyScheme::into_proxy_scheme)),
        }))
    }

    pub(crate) fn system() -> Proxy {
        if cfg!(feature = "__internal_proxy_sys_no_cache") {
            Proxy::new(Intercept::System(Arc::new(get_sys_proxies())))
        } else {
            Proxy::new(Intercept::System(SYS_PROXIES.clone()))
        }
    }

    fn new(intercept: Intercept) -> Proxy {
        Proxy { intercept }
    }

    /// Set the `Proxy-Authorization` header using Basic auth.
    ///
    /// # Example
    ///
    /// ```
    /// # extern crate reqwest;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
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
            Intercept::All(ProxyScheme::Http { ref auth, .. })
            | Intercept::Http(ProxyScheme::Http { ref auth, .. }) => auth.clone(),
            Intercept::Custom(ref custom) => custom.call(uri).and_then(|scheme| match scheme {
                ProxyScheme::Http { auth, .. } => auth,
                ProxyScheme::Https { auth, .. } => auth,
                #[cfg(feature = "socks")]
                _ => None,
            }),
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
            }
            Intercept::Https(ref u) => {
                if uri.scheme() == "https" {
                    Some(u.clone())
                } else {
                    None
                }
            }
            Intercept::System(ref map) => {
                map.get(uri.scheme()).cloned()
            }
            Intercept::Custom(ref custom) => custom.call(uri),
        }
    }

    pub(crate) fn is_match<D: Dst>(&self, uri: &D) -> bool {
        match self.intercept {
            Intercept::All(_) => true,
            Intercept::Http(_) => uri.scheme() == "http",
            Intercept::Https(_) => uri.scheme() == "https",
            Intercept::System(ref map) => map.contains_key(uri.scheme()),
            Intercept::Custom(ref custom) => custom.call(uri).is_some(),
        }
    }
}

impl fmt::Debug for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Proxy").field(&self.intercept).finish()
    }
}

impl ProxyScheme {
    // To start conservative, keep builders private for now.

    /// Proxy traffic via the specified URL over HTTP
    fn http(host: &str) -> crate::Result<Self> {
        Ok(ProxyScheme::Http {
            auth: None,
            host: host.parse().map_err(crate::error::builder)?,
        })
    }

    /// Proxy traffic via the specified URL over HTTPS
    fn https(host: &str) -> crate::Result<Self> {
        Ok(ProxyScheme::Https {
            auth: None,
            host: host.parse().map_err(crate::error::builder)?,
        })
    }

    /// Proxy traffic via the specified socket address over SOCKS5
    ///
    /// # Note
    ///
    /// Current SOCKS5 support is provided via blocking IO.
    #[cfg(feature = "socks")]
    fn socks5(addr: SocketAddr) -> crate::Result<Self> {
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
    fn socks5h(addr: SocketAddr) -> crate::Result<Self> {
        Ok(ProxyScheme::Socks5 {
            addr,
            auth: None,
            remote_dns: true,
        })
    }

    /// Use a username and password when connecting to the proxy server
    fn with_basic_auth<T: Into<String>, U: Into<String>>(
        mut self,
        username: T,
        password: U,
    ) -> Self {
        self.set_basic_auth(username, password);
        self
    }

    fn set_basic_auth<T: Into<String>, U: Into<String>>(&mut self, username: T, password: U) {
        match *self {
            ProxyScheme::Http { ref mut auth, .. } => {
                let header = encode_basic_auth(&username.into(), &password.into());
                *auth = Some(header);
            },
            ProxyScheme::Https { ref mut auth, .. } => {
                let header = encode_basic_auth(&username.into(), &password.into());
                *auth = Some(header);
            },
            #[cfg(feature = "socks")]
            ProxyScheme::Socks5 { ref mut auth, .. } => {
                *auth = Some((username.into(), password.into()));
            }
        }
    }

    fn if_no_auth(mut self, update: &Option<HeaderValue>) -> Self {
        match self {
            ProxyScheme::Http { ref mut auth, .. } => {
                if auth.is_none() {
                    *auth = update.clone();
                }
            },
            ProxyScheme::Https { ref mut auth, .. } => {
                if auth.is_none() {
                    *auth = update.clone();
                }
            },
            #[cfg(feature = "socks")]
            ProxyScheme::Socks5 { .. } => {}
        }

        self
    }

    /// Convert a URL into a proxy scheme
    ///
    /// Supported schemes: HTTP, HTTPS, (SOCKS5, SOCKS5H if `socks` feature is enabled).
    // Private for now...
    fn parse(url: Url) -> crate::Result<Self> {
        use url::Position;

        // Resolve URL to a host and port
        #[cfg(feature = "socks")]
        let to_addr = || {
            let addrs = url.socket_addrs(|| match url.scheme() {
                "socks5" | "socks5h" => Some(1080),
                _ => None,
            })
                .map_err(crate::error::builder)?;
            addrs
                .into_iter()
                .next()
                .ok_or_else(|| crate::error::builder("unknown proxy scheme"))
        };

        let mut scheme = match url.scheme() {
            "http" => Self::http(&url[Position::BeforeHost..Position::AfterPort])?,
            "https" => Self::https(&url[Position::BeforeHost..Position::AfterPort])?,
            #[cfg(feature = "socks")]
            "socks5" => Self::socks5(to_addr()?)?,
            #[cfg(feature = "socks")]
            "socks5h" => Self::socks5h(to_addr()?)?,
            _ => return Err(crate::error::builder("unknown proxy scheme")),
        };

        if let Some(pwd) = url.password() {
            let decoded_username = percent_decode(url.username().as_bytes()).decode_utf8_lossy();
            let decoded_password = percent_decode(pwd.as_bytes()).decode_utf8_lossy();
            scheme = scheme.with_basic_auth(decoded_username, decoded_password);
        }

        Ok(scheme)
    }

    #[cfg(test)]
    fn scheme(&self) -> &str {
        match self {
            ProxyScheme::Http { .. } => "http",
            ProxyScheme::Https { .. } => "https",
            #[cfg(feature = "socks")]
            ProxyScheme::Socks5 { .. } => "socks5",
        }
    }

    #[cfg(test)]
    fn host(&self) -> &str {
        match self {
            ProxyScheme::Http { host, .. } => host.as_str(),
            ProxyScheme::Https { host, .. } => host.as_str(),
            #[cfg(feature = "socks")]
            ProxyScheme::Socks5 { .. } => panic!("socks5"),
        }
    }
}

type SystemProxyMap = HashMap<String, ProxyScheme>;

#[derive(Clone, Debug)]
enum Intercept {
    All(ProxyScheme),
    Http(ProxyScheme),
    Https(ProxyScheme),
    System(Arc<SystemProxyMap>),
    Custom(Custom),
}

impl Intercept {
    fn set_basic_auth(&mut self, username: &str, password: &str) {
        match self {
            Intercept::All(ref mut s)
            | Intercept::Http(ref mut s)
            | Intercept::Https(ref mut s) => s.set_basic_auth(username, password),
            Intercept::System(_) => unimplemented!(),
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
    func: Arc<dyn Fn(&Url) -> Option<crate::Result<ProxyScheme>> + Send + Sync + 'static>,
}

impl Custom {
    fn call<D: Dst>(&self, uri: &D) -> Option<ProxyScheme> {
        let url = format!(
            "{}://{}{}{}",
            uri.scheme(),
            uri.host(),
            uri.port().map(|_| ":").unwrap_or(""),
            uri.port().map(|p| p.to_string()).unwrap_or_default()
        )
        .parse()
        .expect("should be valid Url");

        (self.func)(&url)
            .and_then(|result| result.ok())
            .map(|scheme| scheme.if_no_auth(&self.auth))
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
impl Dst for Uri {
    fn scheme(&self) -> &str {
        self.scheme()
            .expect("Uri should have a scheme")
            .as_str()
    }

    fn host(&self) -> &str {
        Uri::host(self).expect("<Uri as Dst>::host should have a str")
    }

    fn port(&self) -> Option<u16> {
        self.port().map(|p| p.as_u16())
    }
}

lazy_static! {
    static ref SYS_PROXIES: Arc<SystemProxyMap> = Arc::new(get_sys_proxies());
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
fn get_sys_proxies() -> SystemProxyMap {
    let proxies = get_from_environment();

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

fn insert_proxy(proxies: &mut SystemProxyMap, scheme: impl Into<String>, addr: String) -> bool {
    if let Ok(valid_addr) = addr.into_proxy_scheme() {
        proxies.insert(scheme.into(), valid_addr);
        true
    } else {
        false
    }
}

fn get_from_environment() -> SystemProxyMap {
    let mut proxies = HashMap::new();

    if is_cgi() {
        if log::log_enabled!(log::Level::Warn) {
            if env::var_os("HTTP_PROXY").is_some() {
                log::warn!("HTTP_PROXY environment variable ignored in CGI");
            }
        }
    } else if !insert_from_env(&mut proxies, "http", "HTTP_PROXY") {
        insert_from_env(&mut proxies, "http", "http_proxy");
    }

    if !insert_from_env(&mut proxies, "https", "HTTPS_PROXY") {
        insert_from_env(&mut proxies, "https", "https_proxy");
    }

    proxies
}

fn insert_from_env(proxies: &mut SystemProxyMap, scheme: &str, var: &str) -> bool {
    if let Ok(val) = env::var(var) {
        insert_proxy(proxies, scheme, val)
    } else {
        false
    }
}

/// Check if we are being executed in a CGI context.
///
/// If so, a malicious client can send the `Proxy:` header, and it will
/// be in the `HTTP_PROXY` env var. So we don't use it :)
fn is_cgi() -> bool {
    env::var_os("REQUEST_METHOD").is_some()
}

#[cfg(target_os = "windows")]
fn get_from_registry_impl() -> Result<SystemProxyMap, Box<dyn Error>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_setting: RegKey =
        hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")?;
    // ensure the proxy is enable, if the value doesn't exist, an error will returned.
    let proxy_enable: u32 = internet_setting.get_value("ProxyEnable")?;
    let proxy_server: String = internet_setting.get_value("ProxyServer")?;

    if proxy_enable == 0 {
        return Ok(HashMap::new());
    }

    let mut proxies = HashMap::new();
    if proxy_server.contains("=") {
        // per-protocol settings.
        for p in proxy_server.split(";") {
            let protocol_parts: Vec<&str> = p.split("=").collect();
            match protocol_parts.as_slice() {
                [protocol, address] => {
                    insert_proxy(
                        &mut proxies,
                        *protocol,
                        String::from(*address),
                    );
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
            insert_proxy(&mut proxies, "http", proxy_server);
        } else {
            insert_proxy(
                &mut proxies,
                "http",
                format!("http://{}", proxy_server),
            );
            insert_proxy(
                &mut proxies,
                "https",
                format!("https://{}", proxy_server),
            );
        }
    }
    Ok(proxies)
}

#[cfg(target_os = "windows")]
fn get_from_registry() -> SystemProxyMap {
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
            Url::host_str(self).expect("<Url as Dst>::host should have a str")
        }

        fn port(&self) -> Option<u16> {
            Url::port(self)
        }
    }

    fn url(s: &str) -> Url {
        s.parse().unwrap()
    }

    fn intercepted_uri(p: &Proxy, s: &str) -> Uri {
        let (scheme, host) = match p.intercept(&url(s)).unwrap() {
            ProxyScheme::Http { host, .. } => ("http", host),
            ProxyScheme::Https { host, .. } => ("https", host),
            #[cfg(feature = "socks")]
            _ => panic!("intercepted as socks"),
        };
        http::Uri::builder()
            .scheme(scheme)
            .authority(host)
            .path_and_query("/")
            .build()
            .expect("intercepted_uri")
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
    fn test_proxy_scheme_parse() {
        let ps = "http://foo:bar@localhost:1239".into_proxy_scheme().unwrap();

        match ps {
            ProxyScheme::Http { auth, host } => {
                assert_eq!(auth.unwrap(), encode_basic_auth("foo", "bar"));
                assert_eq!(host, "localhost:1239");
            },
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_get_sys_proxies_parsing() {
        // save system setting first.
        let _g1 = env_guard("HTTP_PROXY");
        let _g2 = env_guard("http_proxy");

        // empty
        assert_eq!(get_sys_proxies().contains_key("http"), false);

        // the system proxy setting url is invalid.
        env::set_var("http_proxy", "123465");
        assert_eq!(get_sys_proxies().contains_key("http"), false);

        // set valid proxy
        env::set_var("http_proxy", "http://127.0.0.1/");
        let proxies = get_sys_proxies();

        let p = &proxies["http"];
        assert_eq!(p.scheme(), "http");
        assert_eq!(p.host(), "127.0.0.1");

        // reset user setting when guards drop
    }

    #[test]
    fn test_get_sys_proxies_in_cgi() {
        // save system setting first.
        let _g1 = env_guard("REQUEST_METHOD");
        let _g2 = env_guard("HTTP_PROXY");


        env::set_var("HTTP_PROXY", "http://evil/");

        // not in CGI yet
        assert_eq!(get_sys_proxies()["http"].host(), "evil");

        // set like we're in CGI
        env::set_var("REQUEST_METHOD", "GET");
        assert!(!get_sys_proxies().contains_key("http"));

        // reset user setting when guards drop
    }

    /// Guard an environment variable, resetting it to the original value
    /// when dropped.
    fn env_guard(name: impl Into<String>) -> EnvGuard {
        let name = name.into();
        let orig_val = env::var(&name).ok();
        env::remove_var(&name);
        EnvGuard { name, orig_val }
    }

    struct EnvGuard {
        name: String,
        orig_val: Option<String>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(val) = self.orig_val.take() {
                env::set_var(&self.name, val);
            } else {
                env::remove_var(&self.name);
            }
        }
    }
}
