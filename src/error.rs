use std::error::Error as StdError;
use std::fmt;

/// The Errors that may occur when processing a `Request`.
#[derive(Debug)]
pub enum Error {
    /// An HTTP error from the `hyper` crate.
    Http(::hyper::Error),
    /// An error trying to serialize a value.
    ///
    /// This may be serializing a value that is illegal in JSON or
    /// form-url-encoded bodies.
    Serialize(Box<StdError + Send + Sync>),
    /// A request tried to redirect too many times.
    TooManyRedirects,
    /// An infinite redirect loop was detected.
    RedirectLoop,
    #[doc(hidden)]
    __DontMatchMe,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Http(ref e) => fmt::Display::fmt(e, f),
            Error::Serialize(ref e) => fmt::Display::fmt(e, f),
            Error::TooManyRedirects => f.pad("Too many redirects"),
            Error::RedirectLoop => f.pad("Infinite redirect loop"),
            Error::__DontMatchMe => unreachable!()
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Http(ref e) => e.description(),
            Error::Serialize(ref e) => e.description(),
            Error::TooManyRedirects => "Too many redirects",
            Error::RedirectLoop => "Infinite redirect loop",
            Error::__DontMatchMe => unreachable!()
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match *self {
            Error::Http(ref e) => Some(e),
            Error::Serialize(ref e) => Some(&**e),
            Error::TooManyRedirects => None,
            Error::RedirectLoop => None,
            Error::__DontMatchMe => unreachable!()
        }
    }
}

fn _assert_types() {
    fn _assert_send<T: Send>() {
    }
    _assert_send::<Error>();
}

impl From<::hyper::Error> for Error {
    fn from(err: ::hyper::Error) -> Error {
        Error::Http(err)
    }
}

impl From<::url::ParseError> for Error {
    fn from(err: ::url::ParseError) -> Error {
        Error::Http(::hyper::Error::Uri(err))
    }
}

impl From<::serde_urlencoded::ser::Error> for Error {
    fn from(err: ::serde_urlencoded::ser::Error) -> Error {
        Error::Serialize(Box::new(err))
    }
}

impl From<::serde_json::Error> for Error {
    fn from(err: ::serde_json::Error) -> Error {
        Error::Serialize(Box::new(err))
    }
}

/// A `Result` alias where the `Err` case is `reqwest::Error`.
pub type Result<T> = ::std::result::Result<T, Error>;
