use std::error::Error as StdError;
use std::fmt;
use std::io;

use {StatusCode, Url};

/// The Errors that may occur when processing a `Request`.
///
/// # Examples
///
/// ```
/// extern crate serde;
/// extern crate reqwest;
///
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Simple {
///    key: String
/// }
/// # fn main() { }
///
/// fn run() {
///    match make_request() {
///        Err(e) => handler(e),
///        Ok(_)  => return,
///    }
/// }
/// // Response is not a json object conforming to the Simple struct
/// fn make_request() -> Result<Simple, reqwest::Error> {
///   reqwest::get("http://httpbin.org/ip")?.json()
/// }
///
/// fn handler(e: reqwest::Error) {
///    if e.is_http() {
///        match e.url() {
///            None => println!("No Url given"),
///            Some(url) => println!("Problem making request to: {}", url),
///        }
///    }
///    // Inspect the internal error and output it
///    if e.is_serialization() {
///       let serde_error = match e.get_ref() {
///            None => return,
///            Some(err) => err,
///        };
///        println!("problem parsing information {}", serde_error);
///    }
///    if e.is_redirect() {
///        println!("server redirecting too many times or making loop");
///    }
/// }
/// ```
pub struct Error {
    inner: Box<Inner>,
}

struct Inner {
    kind: Kind,
    url: Option<Url>,
}


/// A `Result` alias where the `Err` case is `reqwest::Error`.
pub type Result<T> = ::std::result::Result<T, Error>;

impl Error {
    fn new(kind: Kind, url: Option<Url>) -> Error {
        Error {
            inner: Box::new(Inner {
                kind,
                url,
            }),
        }
    }

    /// Returns a possible URL related to this error.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn run() {
    /// // displays last stop of a redirect loop
    /// let response = reqwest::get("http://site.with.redirect.loop");
    /// if let Err(e) = response {
    ///     if e.is_redirect() {
    ///         if let Some(final_stop) = e.url() {
    ///             println!("redirect loop at {}", final_stop);
    ///         }
    ///     }
    /// }
    /// # }
    /// ```
    #[inline]
    pub fn url(&self) -> Option<&Url> {
        self.inner.url.as_ref()
    }

    pub(crate) fn with_url(mut self, url: Url) -> Error {
        debug_assert_eq!(self.inner.url, None, "with_url overriding existing url");
        self.inner.url = Some(url);
        self
    }

    /// Returns a reference to the internal error, if available.
    ///
    /// The `'static` bounds allows using `downcast_ref` to check the
    /// details of the error.
    ///
    /// # Examples
    ///
    /// ```
    /// extern crate url;
    /// # extern crate reqwest;
    /// // retries requests with no host on localhost
    /// # fn run() {
    /// let invalid_request = "http://";
    /// let mut response = reqwest::get(invalid_request);
    /// if let Err(e) = response {
    ///     match e.get_ref().and_then(|e| e.downcast_ref::<url::ParseError>()) {
    ///         Some(&url::ParseError::EmptyHost) => {
    ///             let valid_request = format!("{}{}",invalid_request, "localhost");
    ///             response = reqwest::get(&valid_request);
    ///         },
    ///         _ => (),
    ///     }
    /// }
    /// # }
    /// # fn main() {}
    /// ```
    #[inline]
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match self.inner.kind {
            Kind::Http(ref e) => Some(e),
            Kind::Hyper(ref e) => Some(e),
            Kind::Mime(ref e) => Some(e),
            Kind::Url(ref e) => Some(e),
            #[cfg(all(feature = "default-tls", feature = "rustls-tls"))]
            Kind::TlsIncompatible => None,
            #[cfg(feature = "default-tls")]
            Kind::NativeTls(ref e) => Some(e),
            #[cfg(feature = "rustls-tls")]
            Kind::Rustls(ref e) => Some(e),
            #[cfg(feature = "trust-dns")]
            Kind::DnsSystemConf(ref e) => Some(e),
            Kind::Io(ref e) => Some(e),
            Kind::UrlEncoded(ref e) => Some(e),
            Kind::Json(ref e) => Some(e),
            Kind::UrlBadScheme |
            Kind::TooManyRedirects |
            Kind::RedirectLoop |
            Kind::Status(_) |
            Kind::UnknownProxyScheme |
            Kind::Timer => None,
        }
    }

    /// Returns true if the error is related to HTTP.
    #[inline]
    pub fn is_http(&self) -> bool {
        match self.inner.kind {
            Kind::Http(_) => true,
            Kind::Hyper(_) => true,
            _ => false,
        }
    }

    /// Returns true if the error is related to a timeout.
    pub fn is_timeout(&self) -> bool {
        match self.inner.kind {
            Kind::Io(ref io) => io.kind() == io::ErrorKind::TimedOut,
            Kind::Hyper(ref error) => {
                error
                    .source()
                    .and_then(|cause| {
                        cause.downcast_ref::<io::Error>()
                    })
                    .map(|io| io.kind() == io::ErrorKind::TimedOut)
                    .unwrap_or(false)
            },
            _ => false,
        }
    }

    /// Returns true if the error is serialization related.
    #[inline]
    pub fn is_serialization(&self) -> bool {
        match self.inner.kind {
            Kind::Json(_) |
            Kind::UrlEncoded(_) => true,
            _ => false,
        }
    }

    /// Returns true if the error is from a `RedirectPolicy`.
    #[inline]
    pub fn is_redirect(&self) -> bool {
        match self.inner.kind {
            Kind::TooManyRedirects |
            Kind::RedirectLoop => true,
            _ => false,
        }
    }

    /// Returns true if the error is from a request returning a 4xx error.
    #[inline]
    pub fn is_client_error(&self) -> bool {
        match self.inner.kind {
            Kind::Status(code) => code.is_client_error(),
            _ => false,
        }
    }

    /// Returns true if the error is from a request returning a 5xx error.
    #[inline]
    pub fn is_server_error(&self) -> bool {
        match self.inner.kind {
            Kind::Status(code) => code.is_server_error(),
            _ => false,
        }
    }

    /// Returns the status code, if the error was generated from a response.
    #[inline]
    pub fn status(&self) -> Option<StatusCode> {
        match self.inner.kind {
            Kind::Status(code) => Some(code),
            _ => None,
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref url) = self.inner.url {
            f.debug_tuple("Error")
                .field(&self.inner.kind)
                .field(url)
                .finish()
        } else {
            f.debug_tuple("Error")
                .field(&self.inner.kind)
                .finish()
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref url) = self.inner.url {
            fmt::Display::fmt(url, f)?;
            f.write_str(": ")?;
        }
        match self.inner.kind {
            Kind::Http(ref e) => fmt::Display::fmt(e, f),
            Kind::Hyper(ref e) => fmt::Display::fmt(e, f),
            Kind::Mime(ref e) => fmt::Display::fmt(e, f),
            Kind::Url(ref e) => fmt::Display::fmt(e, f),
            Kind::UrlBadScheme => f.write_str("URL scheme is not allowed"),
            #[cfg(all(feature = "default-tls", feature = "rustls-tls"))]
            Kind::TlsIncompatible => f.write_str("Incompatible TLS identity type"),
            #[cfg(feature = "default-tls")]
            Kind::NativeTls(ref e) => fmt::Display::fmt(e, f),
            #[cfg(feature = "rustls-tls")]
            Kind::Rustls(ref e) => fmt::Display::fmt(e, f),
            #[cfg(feature = "trust-dns")]
            Kind::DnsSystemConf(ref e) => {
                write!(f, "failed to load DNS system conf: {}", e)
            },
            Kind::Io(ref e) => fmt::Display::fmt(e, f),
            Kind::UrlEncoded(ref e) => fmt::Display::fmt(e, f),
            Kind::Json(ref e) => fmt::Display::fmt(e, f),
            Kind::TooManyRedirects => f.write_str("Too many redirects"),
            Kind::RedirectLoop => f.write_str("Infinite redirect loop"),
            Kind::Status(ref code) => {
                let prefix = if code.is_client_error() {
                    "Client Error"
                } else if code.is_server_error() {
                    "Server Error"
                } else {
                    unreachable!("non-error status code: {:?}", code);
                };
                write!(f, "{}: {}", prefix, code)
            }
            Kind::UnknownProxyScheme => f.write_str("Unknown proxy scheme"),
            Kind::Timer => f.write_str("timer unavailable"),
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match self.inner.kind {
            Kind::Http(ref e) => e.description(),
            Kind::Hyper(ref e) => e.description(),
            Kind::Mime(ref e) => e.description(),
            Kind::Url(ref e) => e.description(),
            Kind::UrlBadScheme => "URL scheme is not allowed",
            #[cfg(all(feature = "default-tls", feature = "rustls-tls"))]
            Kind::TlsIncompatible => "Incompatible TLS identity type",
            #[cfg(feature = "default-tls")]
            Kind::NativeTls(ref e) => e.description(),
            #[cfg(feature = "rustls-tls")]
            Kind::Rustls(ref e) => e.description(),
            #[cfg(feature = "trust-dns")]
            Kind::DnsSystemConf(_) => "failed to load DNS system conf",
            Kind::Io(ref e) => e.description(),
            Kind::UrlEncoded(ref e) => e.description(),
            Kind::Json(ref e) => e.description(),
            Kind::TooManyRedirects => "Too many redirects",
            Kind::RedirectLoop => "Infinite redirect loop",
            Kind::Status(code) => {
                if code.is_client_error() {
                    "Client Error"
                } else if code.is_server_error() {
                    "Server Error"
                } else {
                    unreachable!("non-error status code: {:?}", code);
                }
            }
            Kind::UnknownProxyScheme => "Unknown proxy scheme",
            Kind::Timer => "timer unavailable",
        }
    }

    // Keep this for now, as std::io::Error didn't support source() until 1.35
    #[allow(deprecated)]
    fn cause(&self) -> Option<&dyn StdError> {
        match self.inner.kind {
            Kind::Http(ref e) => e.cause(),
            Kind::Hyper(ref e) => e.cause(),
            Kind::Mime(ref e) => e.cause(),
            Kind::Url(ref e) => e.cause(),
            #[cfg(all(feature = "default-tls", feature = "rustls-tls"))]
            Kind::TlsIncompatible => None,
            #[cfg(feature = "default-tls")]
            Kind::NativeTls(ref e) => e.cause(),
            #[cfg(feature = "rustls-tls")]
            Kind::Rustls(ref e) => e.cause(),
            #[cfg(feature = "trust-dns")]
            Kind::DnsSystemConf(ref e) => e.cause(),
            Kind::Io(ref e) => e.cause(),
            Kind::UrlEncoded(ref e) => e.cause(),
            Kind::Json(ref e) => e.cause(),
            Kind::UrlBadScheme |
            Kind::TooManyRedirects |
            Kind::RedirectLoop |
            Kind::Status(_) |
            Kind::UnknownProxyScheme |
            Kind::Timer => None,
        }
    }

    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self.inner.kind {
            Kind::Http(ref e) => e.source(),
            Kind::Hyper(ref e) => e.source(),
            Kind::Mime(ref e) => e.source(),
            Kind::Url(ref e) => e.source(),
            #[cfg(all(feature = "default-tls", feature = "rustls-tls"))]
            Kind::TlsIncompatible => None,
            #[cfg(feature = "default-tls")]
            Kind::NativeTls(ref e) => e.source(),
            #[cfg(feature = "rustls-tls")]
            Kind::Rustls(ref e) => e.source(),
            #[cfg(feature = "trust-dns")]
            Kind::DnsSystemConf(ref e) => e.source(),
            Kind::Io(ref e) => e.source(),
            Kind::UrlEncoded(ref e) => e.source(),
            Kind::Json(ref e) => e.source(),
            Kind::UrlBadScheme |
            Kind::TooManyRedirects |
            Kind::RedirectLoop |
            Kind::Status(_) |
            Kind::UnknownProxyScheme |
            Kind::Timer => None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum Kind {
    Http(::http::Error),
    Hyper(::hyper::Error),
    Mime(::mime::FromStrError),
    Url(::url::ParseError),
    UrlBadScheme,
    #[cfg(all(feature = "default-tls", feature = "rustls-tls"))]
    TlsIncompatible,
    #[cfg(feature = "default-tls")]
    NativeTls(::native_tls::Error),
    #[cfg(feature = "rustls-tls")]
    Rustls(::rustls::TLSError),
    #[cfg(feature = "trust-dns")]
    DnsSystemConf(io::Error),
    Io(io::Error),
    UrlEncoded(::serde_urlencoded::ser::Error),
    Json(::serde_json::Error),
    TooManyRedirects,
    RedirectLoop,
    Status(StatusCode),
    UnknownProxyScheme,
    Timer,
}


impl From<::http::Error> for Kind {
    #[inline]
    fn from(err: ::http::Error) -> Kind {
        Kind::Http(err)
    }
}

impl From<::hyper::Error> for Kind {
    #[inline]
    fn from(err: ::hyper::Error) -> Kind {
        Kind::Hyper(err)
    }
}

impl From<::mime::FromStrError> for Kind {
    #[inline]
    fn from(err: ::mime::FromStrError) -> Kind {
        Kind::Mime(err)
    }
}

impl From<io::Error> for Kind {
    #[inline]
    fn from(err: io::Error) -> Kind {
        Kind::Io(err)
    }
}

impl From<::url::ParseError> for Kind {
    #[inline]
    fn from(err: ::url::ParseError) -> Kind {
        Kind::Url(err)
    }
}

impl From<::serde_urlencoded::ser::Error> for Kind {
    #[inline]
    fn from(err: ::serde_urlencoded::ser::Error) -> Kind {
        Kind::UrlEncoded(err)
    }
}

impl From<::serde_json::Error> for Kind {
    #[inline]
    fn from(err: ::serde_json::Error) -> Kind {
        Kind::Json(err)
    }
}

#[cfg(feature = "default-tls")]
impl From<::native_tls::Error> for Kind {
    fn from(err: ::native_tls::Error) -> Kind {
        Kind::NativeTls(err)
    }
}

#[cfg(feature = "rustls-tls")]
impl From<::rustls::TLSError> for Kind {
    fn from(err: ::rustls::TLSError) -> Kind {
        Kind::Rustls(err)
    }
}

impl<T> From<::wait::Waited<T>> for Kind
where T: Into<Kind> {
    fn from(err: ::wait::Waited<T>) -> Kind {
        match err {
            ::wait::Waited::TimedOut =>  io_timeout().into(),
            ::wait::Waited::Inner(e) => e.into(),
        }
    }
}

impl From<::tokio::timer::Error> for Kind {
    fn from(_err: ::tokio::timer::Error) -> Kind {
        Kind::Timer
    }
}

fn io_timeout() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "timed out")
}

#[allow(missing_debug_implementations)]
pub(crate) struct InternalFrom<T>(pub T, pub Option<Url>);

#[doc(hidden)] // https://github.com/rust-lang/rust/issues/42323
impl From<InternalFrom<Error>> for Error {
    #[inline]
    fn from(other: InternalFrom<Error>) -> Error {
        other.0
    }
}

#[doc(hidden)] // https://github.com/rust-lang/rust/issues/42323
impl<T> From<InternalFrom<T>> for Error
where
    T: Into<Kind>,
{
    #[inline]
    fn from(other: InternalFrom<T>) -> Error {
        Error::new(other.0.into(), other.1)
    }
}

pub(crate) fn from<T>(err: T) -> Error
where
    T: Into<Kind>,
{
    InternalFrom(err, None).into()
}

pub(crate) fn into_io(e: Error) -> io::Error {
    match e.inner.kind {
        Kind::Io(io) => io,
        _ => io::Error::new(io::ErrorKind::Other, e),
    }
}

pub(crate) fn from_io(e: io::Error) -> Error {
    if e.get_ref().map(|r| r.is::<Error>()).unwrap_or(false) {
        *e
            .into_inner()
            .expect("io::Error::get_ref was Some(_)")
            .downcast::<Error>()
            .expect("StdError::is() was true")
    } else {
        from(e)
    }
}


macro_rules! try_ {
    ($e:expr) => (
        match $e {
            Ok(v) => v,
            Err(err) => {
                return Err(::error::from(err));
            }
        }
    );
    ($e:expr, $url:expr) => (
        match $e {
            Ok(v) => v,
            Err(err) => {
                return Err(::Error::from(::error::InternalFrom(err, Some($url.clone()))));
            }
        }
    )
}

macro_rules! try_io {
    ($e:expr) => (
        match $e {
            Ok(v) => v,
            Err(ref err) if err.kind() == ::std::io::ErrorKind::WouldBlock => {
                return Ok(::futures::Async::NotReady);
            }
            Err(err) => {
                return Err(::error::from_io(err));
            }
        }
    )
}

pub(crate) fn loop_detected(url: Url) -> Error {
    Error::new(Kind::RedirectLoop, Some(url))
}

pub(crate) fn too_many_redirects(url: Url) -> Error {
    Error::new(Kind::TooManyRedirects, Some(url))
}

pub(crate) fn timedout(url: Option<Url>) -> Error {
    Error::new(Kind::Io(io_timeout()), url)
}

pub(crate) fn status_code(url: Url, status: StatusCode) -> Error {
    Error::new(Kind::Status(status), Some(url))
}

pub(crate) fn url_bad_scheme(url: Url) -> Error {
    Error::new(Kind::UrlBadScheme, Some(url))
}

#[cfg(feature = "trust-dns")]
pub(crate) fn dns_system_conf(io: io::Error) -> Error {
    Error::new(Kind::DnsSystemConf(io), None)
}

pub(crate) fn unknown_proxy_scheme() -> Error {
    Error::new(Kind::UnknownProxyScheme, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(deprecated)]
    #[test]
    fn test_cause_chain() {
        #[derive(Debug)]
        struct Chain<T>(Option<T>);

        impl<T: fmt::Display> fmt::Display  for Chain<T> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                if let Some(ref link) = self.0 {
                    write!(f, "chain: {}", link)
                } else {
                    f.write_str("root")
                }
            }
        }

        impl<T: StdError> StdError for Chain<T> {
            fn description(&self) -> &str {
                if self.0.is_some() {
                    "chain"
                } else {
                    "root"
                }
            }
            fn cause(&self) -> Option<&dyn StdError> {
                if let Some(ref e) = self.0 {
                    Some(e)
                } else {
                    None
                }
            }
        }

        let root = Chain(None::<Error>);
        let io = ::std::io::Error::new(::std::io::ErrorKind::Other, root);
        let err = Error::new(Kind::Io(io), None);
        assert!(err.cause().is_none());
        assert_eq!(err.to_string(), "root");


        let root = ::std::io::Error::new(::std::io::ErrorKind::Other, Chain(None::<Error>));
        let link = Chain(Some(root));
        let io = ::std::io::Error::new(::std::io::ErrorKind::Other, link);
        let err = Error::new(Kind::Io(io), None);
        assert!(err.cause().is_some());
        assert_eq!(err.to_string(), "chain: root");
    }

    #[test]
    fn mem_size_of() {
        use std::mem::size_of;
        assert_eq!(size_of::<Error>(), size_of::<usize>());
    }

    #[test]
    fn roundtrip_io_error() {
        let orig = unknown_proxy_scheme();
        // Convert reqwest::Error into an io::Error...
        let io = into_io(orig);
        // Convert that io::Error back into a reqwest::Error...
        let err = from_io(io);
        // It should have pulled out the original, not nested it...
        match err.inner.kind {
            Kind::UnknownProxyScheme => (),
            _ => panic!("{:?}", err),
        }
    }
}
