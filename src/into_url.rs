use url::Url;

/// A trait to try to convert some type into a `Url`.
///
/// This trait is "sealed", such that only types within reqwest can
/// implement it. The reason is that it will eventually be deprecated
/// and removed, when `std::convert::TryFrom` is stabilized.
pub trait IntoUrl: PolyfillTryInto {}

impl<T: PolyfillTryInto> IntoUrl for T {}

pub trait PolyfillTryInto {
    // Besides parsing as a valid `Url`, the `Url` must be a valid
    // `http::Uri`, in that it makes sense to use in a network request.
    fn into_url(self) -> ::Result<Url>;
}

impl PolyfillTryInto for Url {
    fn into_url(self) -> ::Result<Url> {
        if self.has_host() {
            Ok(self)
        } else {
            Err(::error::url_bad_scheme(self))
        }
    }
}

impl<'a> PolyfillTryInto for &'a str {
    fn into_url(self) -> ::Result<Url> {
        try_!(Url::parse(self))
            .into_url()
    }
}

impl<'a> PolyfillTryInto for &'a String {
    fn into_url(self) -> ::Result<Url> {
        (&**self).into_url()
    }
}

pub(crate) fn expect_uri(url: &Url) -> ::hyper::Uri {
    url.as_str().parse().expect("a parsed Url should always be a valid Uri")
}

pub(crate) fn try_uri(url: &Url) -> Option<::hyper::Uri> {
    url.as_str().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_url_file_scheme() {
        let err = "file:///etc/hosts"
            .into_url()
            .unwrap_err();
        assert_eq!(err.to_string(), "file:///etc/hosts: URL scheme is not allowed");
    }
}
