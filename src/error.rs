/// The Errors that may occur when processing a `Request`.
#[derive(Debug)]
pub enum Error {
    /// An HTTP error from the `hyper` crate.
    Http(::hyper::Error),
    #[doc(hidden)]
    __DontMatchMe,
}

impl From<::hyper::Error> for Error {
    fn from(err: ::hyper::Error) -> Error {
        Error::Http(err)
    }
}

/// A `Result` alias where the `Err` case is `reqwest::Error`.
pub type Result<T> = ::std::result::Result<T, Error>;
