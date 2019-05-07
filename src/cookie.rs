//! The cookies module contains types for working with request and response cookies.

use downcast::Any;

use cookie_crate;
use header;
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// Convert a time::Tm time to SystemTime.
fn tm_to_systemtime(tm: ::time::Tm) -> SystemTime {
    let seconds = tm.to_timespec().sec;
    let duration = std::time::Duration::from_secs(seconds.abs() as u64);
    if seconds > 0 {
        SystemTime::UNIX_EPOCH + duration
    } else {
        SystemTime::UNIX_EPOCH - duration
    }
}

/// Error representing a parse failure of a 'Set-Cookie' header.
pub struct CookieParseError(cookie::ParseError);

impl<'a> fmt::Debug for CookieParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> fmt::Display for CookieParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for CookieParseError {}

/// A single HTTP cookie.
pub struct Cookie<'a>(cookie::Cookie<'a>);

impl<'a> fmt::Debug for Cookie<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Cookie<'static> {
    /// Construct a new cookie with the given name and value.
    pub fn new<N, V>(name: N, value: V) -> Self
    where
        N: Into<Cow<'static, str>>,
        V: Into<Cow<'static, str>>,
    {
        Cookie(cookie::Cookie::new(name, value))
    }
}

impl<'a> Cookie<'a> {
    fn parse(value: &'a ::header::HeaderValue) -> Result<Cookie<'a>, CookieParseError> {
        std::str::from_utf8(value.as_bytes())
            .map_err(cookie::ParseError::from)
            .and_then(cookie::Cookie::parse)
            .map_err(CookieParseError)
            .map(Cookie)
    }

    pub(crate) fn into_inner(self) -> cookie::Cookie<'a> {
        self.0
    }

    /// The name of the cookie.
    pub fn name(&self) -> &str {
        self.0.name()
    }

    /// The value of the cookie.
    pub fn value(&self) -> &str {
        self.0.value()
    }

    /// Returns true if the 'HttpOnly' directive is enabled.
    pub fn http_only(&self) -> bool {
        self.0.http_only().unwrap_or(false)
    }

    /// Returns true if the 'Secure' directive is enabled.
    pub fn secure(&self) -> bool {
        self.0.secure().unwrap_or(false)
    }

    /// Returns true if  'SameSite' directive is 'Lax'.
    pub fn same_site_lax(&self) -> bool {
        self.0.same_site() == Some(cookie_crate::SameSite::Lax)
    }

    /// Returns true if  'SameSite' directive is 'Strict'.
    pub fn same_site_strict(&self) -> bool {
        self.0.same_site() == Some(cookie_crate::SameSite::Strict)
    }

    /// Returns the path directive of the cookie, if set.
    pub fn path(&self) -> Option<&str> {
        self.0.path()
    }

    /// Returns the domain directive of the cookie, if set.
    pub fn domain(&self) -> Option<&str> {
        self.0.domain()
    }

    /// Get the Max-Age information.
    pub fn max_age(&self) -> Option<std::time::Duration> {
        self.0.max_age().map(|d| std::time::Duration::new(d.num_seconds() as u64, 0))
    }

    /// The cookie expiration time.
    pub fn expires(&self) -> Option<SystemTime> {
        self.0.expires().map(tm_to_systemtime)
    }
}

pub(crate) fn extract_response_cookies<'a>(
    headers: &'a hyper::HeaderMap,
) -> impl Iterator<Item = Result<Cookie<'a>, CookieParseError>> + 'a {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| Cookie::parse(value))
}

/// A trait representing a `Session` that can store and retrieve cookies
pub trait CookieStorage: Any + Send + Sync + std::fmt::Debug {
    // cannot return impl Trait in trait inherent function, so Box<dyn...> it is
    /// Retrieve the set of cookies allowed for `url`
    fn get_request_cookies(&self, url: &url::Url) -> Box<dyn Iterator<Item = &cookie_crate::Cookie<'static>> + '_>;
    /// Store a set of `cookies` from `url`
    fn store_response_cookies(
        &mut self,
        cookies: Box<dyn Iterator<Item = cookie_crate::Cookie<'static>>>,
        url: &url::Url,
    );

}

// FIXME: how to deal w/ missing docs in macro generated code?
#[allow(missing_docs)]
downcast!(CookieStorage);

impl CookieStorage for cookie_store::CookieStore {
    fn get_request_cookies(&self, url: &url::Url) -> Box<dyn Iterator<Item = &cookie_crate::Cookie<'static>> + '_> {
        Box::new(self.get_request_cookies(url))
    }

    fn store_response_cookies(
        &mut self,
        cookies: Box<dyn Iterator<Item = cookie_crate::Cookie<'static>>>,
        url: &url::Url,
    ) {
        self.store_response_cookies(cookies, url);
    }
}

macro_rules! define_session {
    ($client_ty:ty, $builder_ty:ty, $default_builder:expr) => (
/// A session that provides cookie handling.
#[derive(Debug)]
pub struct Session<S: CookieStorage> {
    cookie_store: Arc<RwLock<Box<dyn CookieStorage>>>,
    client: $client_ty,
    type_holder: PhantomData<S>,
}

impl<S: CookieStorage + 'static> Session<S> {
    /// Create a new session with a default `Client` configuration using
    /// the supplied cookie storage
    pub fn new(cookie_store: S) -> ::Result<Session<S>> {
        Session::<S>::from_builder(cookie_store, $default_builder)
    }

    /// Create a new session with the configured `ClientBuilder`.
    pub fn from_builder(cookie_store: S, client_builder: $builder_ty) -> ::Result<Session<S>> {
        let cookie_store: Arc<RwLock<Box<dyn CookieStorage>>> =
            Arc::new(RwLock::new(Box::new(cookie_store)));
        client_builder
            .cookie_store(cookie_store.clone())
            .build()
            .map(|client| {
                Session {
                    cookie_store,
                    client,
                    type_holder: PhantomData,
                }
            })
    }

    // FIXME: this seems like a bad idea in the face of async behavior? Would only want to do this
    // when there are no requests in flight
    /// Retrieve the currently used cookie storage, replacing it with the provided new instance
    pub fn replace_cookie_store(&mut self, new_store: S) -> S {
        let new_store = Box::new(new_store);
        let mut prior_store = self.cookie_store.write().unwrap();
        let prior_store = std::mem::replace(&mut *prior_store, new_store);
        *prior_store
            .downcast::<S>()
            .expect("failed to downcast back to original type")
    }

    /// Modify the contents of the current cookie storage with `f`
    pub fn modify_cookie_store<F: Fn(&mut S)>(&self, f: F) {
        let mut lock = self.cookie_store.write().unwrap();
        let store_ref = lock.downcast_mut::<S>().unwrap();
        f(store_ref);
    }

    /// End the current session, returning the current cookie storage.
    pub fn end(self) -> S {
        drop(self.client);
        let cookie_store = Arc::try_unwrap(self.cookie_store)
            .map_err(|e| format!("Could not unwrap Arc: {:?}", e))
            .and_then(|r| {
                r.into_inner()
                    .map_err(|e| format!("Could not remove RwLock: {:?}", e))
            })
        .expect("Could not retrieve S");
        *cookie_store
            .downcast::<S>()
            .expect("failed to downcast back to original type")
    }
}

impl<S: CookieStorage> std::ops::Deref for Session<S> {
    type Target = $client_ty;
    fn deref(&self) -> &$client_ty {
        &(*self).client
    }
}
)
}

define_session!(::Client, ::ClientBuilder, ::ClientBuilder::new());
