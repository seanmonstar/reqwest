use url::{Url, ParseError};

/// A trait to try to convert some type into a `Url`.
///
/// This trait is "sealed", such that only types within reqwest can
/// implement it. The reason is that it will eventually be deprecated
/// and removed, when `std::convert::TryFrom` is stabilized.
pub trait IntoUrl: PolyfillTryInto {}

impl<T: PolyfillTryInto> IntoUrl for T {}

// pub(crate)

pub trait PolyfillTryInto {
    fn into_url(self) -> Result<Url, ParseError>;
}

impl PolyfillTryInto for Url {
    fn into_url(self) -> Result<Url, ParseError> {
        Ok(self)
    }
}

impl<'a> PolyfillTryInto for &'a str {
    fn into_url(self) -> Result<Url, ParseError> {
        Url::parse(self)
    }
}

impl<'a> PolyfillTryInto for &'a String {
    fn into_url(self) -> Result<Url, ParseError> {
        Url::parse(self)
    }
}
