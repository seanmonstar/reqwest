use std::error::Error as StdError;
use std::fmt;

/// The Errors that may occur when processing a `Request`.
#[derive(Debug)]
pub enum Error {
    /// An HTTP error from the `hyper` crate.
    Http(::hyper::Error),
    /// A request tried to redirect too many times.
    TooManyRedirects,
    #[doc(hidden)]
    __DontMatchMe,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Http(ref e) => fmt::Display::fmt(e, f),
            Error::TooManyRedirects => {
                f.pad("Too many redirects")
            },
            Error::__DontMatchMe => unreachable!()
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Http(ref e) => e.description(),
            Error::TooManyRedirects => "Too many redirects",
            Error::__DontMatchMe => unreachable!()
        }
    }

    fn cause(&self) -> Option<&StdError> {
        match *self {
            Error::Http(ref e) => Some(e),
            Error::TooManyRedirects => None,
            Error::__DontMatchMe => unreachable!()
        }
    }
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



/// A `Result` alias where the `Err` case is `reqwest::Error`.
pub type Result<T> = ::std::result::Result<T, Error>;
