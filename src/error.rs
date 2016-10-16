#[derive(Debug)]
pub enum Error {
    Http(::hyper::Error),
    #[doc(hidden)]
    __DontMatchMe,
}

impl From<::hyper::Error> for Error {
    fn from(err: ::hyper::Error) -> Error {
        Error::Http(err)
    }
}

pub type Result<T> = ::std::result::Result<T, Error>;
