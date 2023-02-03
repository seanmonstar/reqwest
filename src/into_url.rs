use std::sync::Arc;
use url::Url;

/// A trait to try to convert some type into a `Url`.
///
/// This trait is "sealed", such that only types within reqwest can
/// implement it.
pub trait IntoUrl: IntoUrlSealed {}

impl IntoUrl for Url {}
impl IntoUrl for Arc<Url> {}
impl IntoUrl for String {}
impl IntoUrl for &Arc<Url> {}
impl IntoUrl for &str {}
impl IntoUrl for &String {}

pub trait IntoUrlSealed {
    // Besides parsing as a valid `Url`, the `Url` must be a valid
    // `http::Uri`, in that it makes sense to use in a network request.
    fn into_url(self) -> crate::Result<Arc<Url>>;

    fn as_str(&self) -> &str;
}

impl IntoUrlSealed for Url {
    fn into_url(self) -> crate::Result<Arc<Url>> {
        Arc::new(self).into_url()
    }

    fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl IntoUrlSealed for Arc<Url> {
    fn into_url(self) -> crate::Result<Arc<Url>> {
        if self.has_host() {
            Ok(self)
        } else {
            Err(crate::error::url_bad_scheme(self))
        }
    }

    fn as_str(&self) -> &str {
        (**self).as_ref()
    }
}

impl IntoUrlSealed for &Arc<Url> {
    fn into_url(self) -> crate::Result<Arc<Url>> {
        self.clone().into_url()
    }

    fn as_str(&self) -> &str {
        (***self).as_ref()
    }
}

impl IntoUrlSealed for &str {
    fn into_url(self) -> crate::Result<Arc<Url>> {
        Url::parse(self).map_err(crate::error::builder)?.into_url()
    }

    fn as_str(&self) -> &str {
        self
    }
}

impl IntoUrlSealed for &String {
    fn into_url(self) -> crate::Result<Arc<Url>> {
        (&**self).into_url()
    }

    fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl IntoUrlSealed for String {
    fn into_url(self) -> crate::Result<Arc<Url>> {
        (&*self).into_url()
    }

    fn as_str(&self) -> &str {
        self.as_ref()
    }
}

if_hyper! {
    pub(crate) fn expect_uri(url: &Url) -> http::Uri {
        url.as_str()
            .parse()
            .expect("a parsed Url should always be a valid Uri")
    }

    pub(crate) fn try_uri(url: &Url) -> Option<http::Uri> {
        url.as_str().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_url_file_scheme() {
        let err = "file:///etc/hosts".into_url().unwrap_err();
        assert_eq!(
            err.to_string(),
            "builder error for url (file:///etc/hosts): URL scheme is not allowed"
        );
    }
}
