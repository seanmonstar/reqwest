//! The cookies module contains types for working with request and response cookies.

use cookie_crate;
use header;
use std::borrow::Cow;
use std::fmt;
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

/// Convert a SystemTime to time::Tm.
/// Returns None if the conversion failed.
fn systemtime_to_tm(time: SystemTime) -> Option<::time::Tm> {
    let seconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(_) => {
            if let Ok(duration) = SystemTime::UNIX_EPOCH.duration_since(time) {
                (duration.as_secs() as i64) * -1
            } else {
                return None;
            }
        }
    };
    Some(::time::at_utc(::time::Timespec::new(seconds, 0)))
}

/// Represents the 'SameSite' attribute of a cookie.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum SameSite {
    /// Strict same-site policy.
    Strict,
    /// Lax same-site policy.
    Lax,
}

impl SameSite {
    fn from_inner(value: cookie_crate::SameSite) -> Option<Self> {
        match value {
            cookie_crate::SameSite::Strict => Some(SameSite::Strict),
            cookie_crate::SameSite::Lax => Some(SameSite::Lax),
            cookie_crate::SameSite::None => None,
        }
    }

    fn to_inner(value: Option<Self>) -> cookie_crate::SameSite {
        match value {
            Some(SameSite::Strict) => cookie_crate::SameSite::Strict,
            Some(SameSite::Lax) => cookie_crate::SameSite::Lax,
            None => cookie_crate::SameSite::None,
        }
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
        Self(cookie::Cookie::new(name, value))
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

    /// Set the cookie name.
    pub fn set_name<P: Into<Cow<'static, str>>>(&mut self, name: P) {
        self.0.set_name(name)
    }

    /// The value of the cookie.
    pub fn value(&self) -> &str {
        self.0.value()
    }

    /// Set the cookie value.
    pub fn set_value<P: Into<Cow<'static, str>>>(&mut self, value: P) {
        self.0.set_value(value)
    }

    /// Returns true if the 'HttpOnly' directive is enabled.
    pub fn http_only(&self) -> bool {
        self.0.http_only().unwrap_or(false)
    }

    /// Set the 'HttpOnly' directive.
    pub fn set_http_only(&mut self, value: bool) {
        self.0.set_http_only(value)
    }

    /// Returns true if the 'Secure' directive is enabled.
    pub fn secure(&self) -> bool {
        self.0.secure().unwrap_or(false)
    }

    /// Set the 'Secure' directive.
    pub fn set_secure(&mut self, value: bool) {
        self.0.set_secure(value)
    }

    /// Returns the 'SameSite' directive if present.
    pub fn same_site(&self) -> Option<SameSite> {
        self.0.same_site().and_then(SameSite::from_inner)
    }

    /// Set the 'SameSite" directive.
    pub fn set_same_site(&mut self, value: Option<SameSite>) {
        self.0.set_same_site(SameSite::to_inner(value))
    }

    /// Returns the path directive of the cookie, if set.
    pub fn path(&self) -> Option<&str> {
        self.0.path()
    }

    /// Set the cookie path.
    pub fn set_path<P: Into<Cow<'static, str>>>(&mut self, path: P) {
        self.0.set_path(path)
    }

    /// Returns the domain directive of the cookie, if set.
    pub fn domain(&self) -> Option<&str> {
        self.0.domain()
    }

    /// Set the cookie domain.
    pub fn set_domain<P: Into<Cow<'static, str>>>(&mut self, domain: P) {
        self.0.set_domain(domain)
    }

    /// Get the Max-Age information.
    pub fn max_age(&self) -> Option<std::time::Duration> {
        self.0.max_age().map(|d| std::time::Duration::new(d.num_seconds() as u64, 0))
    }

    /// The cookie expiration time.
    pub fn expires(&self) -> Option<SystemTime> {
        self.0.expires().map(tm_to_systemtime)
    }

    /// Set expiration time.
    ///
    /// Currently, providing a `None` value will have no effect.
    pub fn set_expires(&mut self, value: Option<SystemTime>) {
        if let Some(tm) = value.and_then(systemtime_to_tm) {
            self.0.set_expires(tm);
        }
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

/// A persistent cookie store that provides session support.
#[derive(Default)]
pub(crate) struct CookieStore(pub(crate) ::cookie_store::CookieStore);

impl<'a> fmt::Debug for CookieStore {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
