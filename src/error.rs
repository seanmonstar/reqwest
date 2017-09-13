use std::error::Error as StdError;
use std::fmt;
use std::io;

use {StatusCode, Url};

/// The Errors that may occur when processing a `Request`.
///
/// # Examples
///
/// ```
/// #[macro_use]
/// extern crate serde_derive;
/// extern crate reqwest;
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
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    url: Option<Url>,
}

/// A `Result` alias where the `Err` case is `reqwest::Error`.
pub type Result<T> = ::std::result::Result<T, Error>;

impl Error {
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
        self.url.as_ref()
    }

    /// Returns the corresponding `ErrorKind` for this error.
    ///
    /// # Examples
    ///
    /// ```
    /// extern crate url;
    ///
    /// # extern crate reqwest;
    /// # use reqwest::ErrorKind;
    /// #
    /// # fn run() {
    /// if let Err(e) = reqwest::get("http://") {
    ///     match *e.kind() {
    ///         ErrorKind::RedirectLoop => println!("encountered redirect loop"),
    ///         ErrorKind::Json(_) => println!("json serialization error"),
    ///         _ => println!("some other, unhandled error"),
    ///     }
    /// }
    /// # }
    /// # fn main() { }
    /// ```
    #[inline]
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
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
    pub fn get_ref(&self) -> Option<&(StdError + Send + Sync + 'static)> {
        match self.kind {
            ErrorKind::Http(ref e) => Some(e),
            ErrorKind::Url(ref e) => Some(e),
            ErrorKind::Tls(ref e) => Some(e),
            ErrorKind::Io(ref e) => Some(e),
            ErrorKind::UrlEncoded(ref e) => Some(e),
            ErrorKind::Json(ref e) => Some(e),
            ErrorKind::TooManyRedirects |
            ErrorKind::RedirectLoop |
            ErrorKind::ClientError(_) |
            ErrorKind::ServerError(_) => None,
        }
    }

    /// Returns true if the error is related to HTTP.
    #[inline]
    pub fn is_http(&self) -> bool {
        match self.kind {
            ErrorKind::Http(_) => true,
            _ => false,
        }
    }

    /// Returns true if the error is serialization related.
    #[inline]
    pub fn is_serialization(&self) -> bool {
        match self.kind {
            ErrorKind::Json(_) |
            ErrorKind::UrlEncoded(_) => true,
            _ => false,
        }
    }

    /// Returns true if the error is from a `RedirectPolicy`.
    #[inline]
    pub fn is_redirect(&self) -> bool {
        match self.kind {
            ErrorKind::TooManyRedirects |
            ErrorKind::RedirectLoop => true,
            _ => false,
        }
    }

    /// Returns true if the error is from a request returning a 4xx error.
    #[inline]
    pub fn is_client_error(&self) -> bool {
        match self.kind {
            ErrorKind::ClientError(_) => true,
            _ => false,
        }
    }

    /// Returns true if the error is from a request returning a 5xx error.
    #[inline]
    pub fn is_server_error(&self) -> bool {
        match self.kind {
            ErrorKind::ServerError(_) => true,
            _ => false,
        }
    }

    /// Returns the status code, if the error was generated from a response.
    #[inline]
    pub fn status(&self) -> Option<StatusCode> {
        match self.kind {
            ErrorKind::ClientError(code) |
            ErrorKind::ServerError(code) => Some(code),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref url) = self.url {
            try!(fmt::Display::fmt(url, f));
            try!(f.write_str(": "));
        }
        match self.kind {
            ErrorKind::Http(ref e) => fmt::Display::fmt(e, f),
            ErrorKind::Url(ref e) => fmt::Display::fmt(e, f),
            ErrorKind::Tls(ref e) => fmt::Display::fmt(e, f),
            ErrorKind::Io(ref e) => fmt::Display::fmt(e, f),
            ErrorKind::UrlEncoded(ref e) => fmt::Display::fmt(e, f),
            ErrorKind::Json(ref e) => fmt::Display::fmt(e, f),
            ErrorKind::TooManyRedirects => f.write_str("Too many redirects"),
            ErrorKind::RedirectLoop => f.write_str("Infinite redirect loop"),
            ErrorKind::ClientError(ref code) => {
                f.write_str("Client Error: ")?;
                fmt::Display::fmt(code, f)
            }
            ErrorKind::ServerError(ref code) => {
                f.write_str("Server Error: ")?;
                fmt::Display::fmt(code, f)
            }
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match self.kind {
            ErrorKind::Http(ref e) => e.description(),
            ErrorKind::Url(ref e) => e.description(),
            ErrorKind::Tls(ref e) => e.description(),
            ErrorKind::Io(ref e) => e.description(),
            ErrorKind::UrlEncoded(ref e) => e.description(),
            ErrorKind::Json(ref e) => e.description(),
            ErrorKind::TooManyRedirects => "Too many redirects",
            ErrorKind::RedirectLoop => "Infinite redirect loop",
            ErrorKind::ClientError(_) => "Client Error",
            ErrorKind::ServerError(_) => "Server Error",
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match self.kind {
            ErrorKind::Http(ref e) => e.cause(),
            ErrorKind::Url(ref e) => e.cause(),
            ErrorKind::Tls(ref e) => e.cause(),
            ErrorKind::Io(ref e) => e.cause(),
            ErrorKind::UrlEncoded(ref e) => e.cause(),
            ErrorKind::Json(ref e) => e.cause(),
            ErrorKind::TooManyRedirects |
            ErrorKind::RedirectLoop |
            ErrorKind::ClientError(_) |
            ErrorKind::ServerError(_) => None,
        }
    }
}


/// An exhaustive list of errors that may be encountered when using reqwest.
///
/// This list is intended to grow over time.
#[derive(Debug)]
pub enum ErrorKind {
    /// todo: pending pr comments
    Http(::hyper::Error),
    /// todo: pending pr comments
    Url(::url::ParseError),
    /// todo: pending pr comments
    Tls(::native_tls::Error),
    /// todo: pending pr comments
    Io(io::Error),
    /// todo: pending pr comments
    UrlEncoded(::serde_urlencoded::ser::Error),
    /// todo: pending pr comments
    Json(::serde_json::Error),
    /// todo: pending pr comments
    TooManyRedirects,
    /// todo: pending pr comments
    RedirectLoop,
    /// todo: pending pr comments
    ClientError(StatusCode),
    /// todo: pending pr comments
    ServerError(StatusCode),
}


impl From<::hyper::Error> for ErrorKind {
    #[inline]
    fn from(err: ::hyper::Error) -> ErrorKind {
        match err {
            ::hyper::Error::Io(err) => ErrorKind::Io(err),
            //::hyper::Error::Uri(err) => ErrorKind::Url(err),
            other => ErrorKind::Http(other),
        }
    }
}

impl From<io::Error> for ErrorKind {
    #[inline]
    fn from(err: io::Error) -> ErrorKind {
        ErrorKind::Io(err)
    }
}

impl From<::url::ParseError> for ErrorKind {
    #[inline]
    fn from(err: ::url::ParseError) -> ErrorKind {
        ErrorKind::Url(err)
    }
}

impl From<::serde_urlencoded::ser::Error> for ErrorKind {
    #[inline]
    fn from(err: ::serde_urlencoded::ser::Error) -> ErrorKind {
        ErrorKind::UrlEncoded(err)
    }
}

impl From<::serde_json::Error> for ErrorKind {
    #[inline]
    fn from(err: ::serde_json::Error) -> ErrorKind {
        ErrorKind::Json(err)
    }
}

impl From<::native_tls::Error> for ErrorKind {
    fn from(err: ::native_tls::Error) -> ErrorKind {
        ErrorKind::Tls(err)
    }
}

impl<T> From<::wait::Waited<T>> for ErrorKind
where T: Into<ErrorKind> {
    fn from(err: ::wait::Waited<T>) -> ErrorKind {
        match err {
            ::wait::Waited::TimedOut =>  io_timeout().into(),
            ::wait::Waited::Err(e) => e.into(),
        }
    }
}

#[cfg(unix)]
fn io_timeout() -> io::Error {
    io::Error::new(io::ErrorKind::WouldBlock, "timed out")
}

#[cfg(windows)]
fn io_timeout() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "timed out")
}

// pub(crate)

#[allow(missing_debug_implementations)]
pub struct InternalFrom<T>(pub T, pub Option<Url>);

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
    T: Into<ErrorKind>,
{
    #[inline]
    fn from(other: InternalFrom<T>) -> Error {
        Error {
            kind: other.0.into(),
            url: other.1,
        }
    }
}

#[inline]
pub fn from<T>(err: T) -> Error
where
    T: Into<ErrorKind>,
{
    InternalFrom(err, None).into()
}

#[inline]
pub fn into_io(e: Error) -> io::Error {
    match e.kind {
        ErrorKind::Io(io) => io,
        _ => io::Error::new(io::ErrorKind::Other, e),
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

#[inline]
pub fn loop_detected(url: Url) -> Error {
    Error {
        kind: ErrorKind::RedirectLoop,
        url: Some(url),
    }
}

#[inline]
pub fn too_many_redirects(url: Url) -> Error {
    Error {
        kind: ErrorKind::TooManyRedirects,
        url: Some(url),
    }
}

#[inline]
pub fn timedout(url: Option<Url>) -> Error {
    Error {
        kind: ErrorKind::Io(io_timeout()),
        url: url,
    }
}

#[inline]
pub fn client_error(url: Url, status: StatusCode) -> Error {
    Error {
        kind: ErrorKind::ClientError(status),
        url: Some(url),
    }
}

#[inline]
pub fn server_error(url: Url, status: StatusCode) -> Error {
    Error {
        kind: ErrorKind::ServerError(status),
        url: Some(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_get_ref_downcasts() {
        let err: Error = from(::hyper::Error::Status);
        let cause = err.get_ref()
            .unwrap()
            .downcast_ref::<::hyper::Error>()
            .unwrap();

        match cause {
            &::hyper::Error::Status => (),
            _ => panic!("unexpected downcast: {:?}", cause),
        }
    }

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
            fn cause(&self) -> Option<&StdError> {
                if let Some(ref e) = self.0 {
                    Some(e)
                } else {
                    None
                }
            }
        }

        let err = from(::hyper::Error::Status);
        assert!(err.cause().is_none());

        let root = Chain(None::<Error>);
        let io = ::std::io::Error::new(::std::io::ErrorKind::Other, root);
        let err = Error { kind: ErrorKind::Io(io), url: None };
        assert!(err.cause().is_none());
        assert_eq!(err.to_string(), "root");


        let root = ::std::io::Error::new(::std::io::ErrorKind::Other, Chain(None::<Error>));
        let link = Chain(Some(root));
        let io = ::std::io::Error::new(::std::io::ErrorKind::Other, link);
        let err = Error { kind: ErrorKind::Io(io), url: None };
        assert!(err.cause().is_some());
        assert_eq!(err.to_string(), "chain: root");
    }
}
