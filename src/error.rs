use std::error::Error as StdError;
use std::fmt;

use Url;

/// The Errors that may occur when processing a `Request`.
#[derive(Debug)]
pub struct Error {
    kind: Kind,
    url: Option<Url>,
}

/// A `Result` alias where the `Err` case is `reqwest::Error`.
pub type Result<T> = ::std::result::Result<T, Error>;

impl Error {
    /// Returns a possible URL related to this error.
    #[inline]
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }

    /// Returns a reference to the internal error, if available.
    ///
    /// The `'static` bounds allows using `downcast_ref` to check the
    /// details of the error.
    #[inline]
    pub fn get_ref(&self) -> Option<&(StdError + Send + Sync + 'static)> {
        match self.kind {
            Kind::Http(ref e) => Some(e),
            Kind::UrlEncoded(ref e) => Some(e),
            Kind::Json(ref e) => Some(e),
            Kind::TooManyRedirects |
            Kind::RedirectLoop => None,
        }
    }

    /// Returns true if the error is related to HTTP.
    #[inline]
    pub fn is_http(&self) -> bool {
        match self.kind {
            Kind::Http(_) => true,
            _ => false,
        }
    }

    /// Returns true if the error is serialization related.
    #[inline]
    pub fn is_serialization(&self) -> bool {
        match self.kind {
            Kind::Json(_) |
            Kind::UrlEncoded(_) => true,
            _ => false,
        }
    }

    /// Returns true if the error is from a `RedirectPolicy`.
    #[inline]
    pub fn is_redirect(&self) -> bool {
        match self.kind {
            Kind::TooManyRedirects |
            Kind::RedirectLoop => true,
            _ => false,
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
            Kind::Http(ref e) => fmt::Display::fmt(e, f),
            Kind::UrlEncoded(ref e) => fmt::Display::fmt(e, f),
            Kind::Json(ref e) => fmt::Display::fmt(e, f),
            Kind::TooManyRedirects => f.write_str("Too many redirects"),
            Kind::RedirectLoop => f.write_str("Infinite redirect loop"),
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match self.kind {
            Kind::Http(ref e) => e.description(),
            Kind::UrlEncoded(ref e) => e.description(),
            Kind::Json(ref e) => e.description(),
            Kind::TooManyRedirects => "Too many redirects",
            Kind::RedirectLoop => "Infinite redirect loop",
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match self.kind {
            Kind::Http(ref e) => Some(e),
            Kind::UrlEncoded(ref e) => Some(e),
            Kind::Json(ref e) => Some(e),
            Kind::TooManyRedirects |
            Kind::RedirectLoop => None,
        }
    }
}

// pub(crate)

#[derive(Debug)]
pub enum Kind {
    Http(::hyper::Error),
    UrlEncoded(::serde_urlencoded::ser::Error),
    Json(::serde_json::Error),
    TooManyRedirects,
    RedirectLoop,
}


impl From<::hyper::Error> for Kind {
    #[inline]
    fn from(err: ::hyper::Error) -> Kind {
        Kind::Http(err)
    }
}

impl From<::url::ParseError> for Kind {
    #[inline]
    fn from(err: ::url::ParseError) -> Kind {
        Kind::Http(::hyper::Error::Uri(err))
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

impl From<::hyper_native_tls::native_tls::Error> for Kind {
    fn from(other: ::hyper_native_tls::native_tls::Error) -> Kind {
        ::hyper::Error::Ssl(Box::new(other)).into()
    }
}


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
    T: Into<Kind>,
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
    T: Into<Kind>,
{
    InternalFrom(err, None).into()
}

#[inline]
pub fn loop_detected(url: Url) -> Error {
    Error {
        kind: Kind::RedirectLoop,
        url: Some(url),
    }
}

#[inline]
pub fn too_many_redirects(url: Url) -> Error {
    Error {
        kind: Kind::TooManyRedirects,
        url: Some(url),
    }
}

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
