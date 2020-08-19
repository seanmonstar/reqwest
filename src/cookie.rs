//! HTTP Cookies

use std::convert::TryInto;

use crate::header;
use std::fmt;
use std::time::SystemTime;

/// A single HTTP cookie.
pub struct Cookie<'a>(cookie_crate::Cookie<'a>);

impl<'a> Cookie<'a> {
    fn parse(value: &'a crate::header::HeaderValue) -> Result<Cookie<'a>, CookieParseError> {
        std::str::from_utf8(value.as_bytes())
            .map_err(cookie_crate::ParseError::from)
            .and_then(cookie_crate::Cookie::parse)
            .map_err(CookieParseError)
            .map(Cookie)
    }

    pub(crate) fn into_inner(self) -> cookie_crate::Cookie<'a> {
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
        self.0
            .max_age()
            .map(|d| d.try_into().expect("time::Duration into std::time::Duration"))
    }

    /// The cookie expiration time.
    pub fn expires(&self) -> Option<SystemTime> {
        self.0.expires().map(SystemTime::from)
    }
}

impl<'a> fmt::Debug for Cookie<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
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
pub struct CookieStore(pub cookie_store::CookieStore);

impl CookieStore {
    /// Return an `Iterator` of the cookies for `url` in the store
    pub fn get_request_cookies(
        &self,
        url: &url::Url
    ) -> impl Iterator<Item = &cookie_crate::Cookie<'static>> {
        self.0.get_request_cookies(url)
    }

    /// Store the `cookies` received from `url`
    pub fn store_response_cookies<I: Iterator<Item = cookie_crate::Cookie<'static>>>(
        &mut self,
        cookies: I,
        url: &crate::Url
    ) {
        self.0.store_response_cookies(cookies, url);
    }

    /// Returns true if the `CookieStore` contains an **unexpired** `Cookie` corresponding to the specified `domain`, `path`, and `name`.
    pub fn contains(&self, domain: &str, path: &str, name: &str) -> bool {
        self.0.contains(domain, path, name)
    }

    /// Returns true if the `CookieStore` contains any (even an **expired**) `Cookie` corresponding to the specified `domain`, `path`, and `name`.
    pub fn contains_any(&self, domain: &str, path: &str, name: &str) -> bool {
        self.0.contains_any(domain, path, name)
    }

    /// Returns a reference to the **unexpired** `Cookie` corresponding to the specified `domain`, `path`, and `name`.
    pub fn get(&self, domain: &str, path: &str, name: &str) -> Option<&cookie_store::Cookie<'_>> {
        self.0.get(domain, path, name)
    }

    /// Returns a reference to the (possibly **expired**) `Cookie` corresponding to the specified `domain`, `path`, and `name`.
    pub fn get_any(&self, domain: &str, path: &str, name: &str) -> Option<&cookie_store::Cookie<'_>> {
        self.0.get_any(domain, path, name)
    }

    /// Removes a `Cookie` from the store, returning the `Cookie` if it was in the store
    pub fn remove(
        &mut self,
        domain: &str,
        path: &str,
        name: &str
    ) -> Option<cookie_store::Cookie<'static>> {
        self.0.remove(domain, path, name)
    }

    /// Returns a collection of references to unexpired cookies that path- and domain-match request_url, as well as having HttpOnly and Secure attributes compatible with the request_url.
    pub fn matches(&self, request_url: &crate::Url) -> Vec<&cookie_store::Cookie<'static>> {
        self.0.matches(request_url)
    }

    /// Parses a new `Cookie` from `cookie_str` and inserts it into the store.
    pub fn parse(
        &mut self,
        cookie_str: &str,
        request_url: &crate::Url
    ) -> Result<(), cookie_store::CookieError> {
        match self.0.parse(cookie_str, request_url) {
            Ok(_store_action) => Ok(()),
            Err(e) => Err(e)
        }
    }

    /// Converts a `cookie::Cookie` (from the `cookie` crate) into a `cookie_store::Cookie` and inserts it into the store.
    pub fn insert_raw(
        &mut self,
        cookie: &cookie_crate::Cookie,
        request_url: &crate::Url
    ) -> Result<(), cookie_store::CookieError> {
        match self.0.insert_raw(cookie, request_url) {
            Ok(_store_action) => Ok(()),
            Err(e) => Err(e)
        }
    }

    /// Inserts `cookie`, received from `request_url`, into the store, following the rules of the
    /// [IETF RFC6265 Storage Model](http://tools.ietf.org/html/rfc6265#section-5.3). If the
    /// `Cookie` is __unexpired__ and is successfully inserted, returns
    /// `Ok(StoreAction::Inserted)`. If the `Cookie` is __expired__ *and* matches an existing
    /// `Cookie` in the store, the existing `Cookie` wil be `expired()` and
    /// `Ok(StoreAction::ExpiredExisting)` will be returned.
    pub fn insert(
        &mut self,
        cookie: cookie_store::Cookie<'static>,
        request_url: &crate::Url
    ) -> Result<(), cookie_store::CookieError> {
        match self.0.insert(cookie, request_url) {
            Ok(_store_action) => Ok(()),
            Err(e) => Err(e)
        }
    }

    /// Clear the contents of the store
    pub fn clear(&mut self) {
        self.0.clear()
    }

    /// An iterator visiting all the **unexpired** cookies in the store
    pub fn iter_unexpired<'a>(
        &'a self
    ) -> impl Iterator<Item = &'a cookie_store::Cookie<'static>> + 'a {
        self.0.iter_unexpired()
    }

    /// An iterator visiting all (including **expired**) cookies in the store
    pub fn iter_any<'a>(&'a self) -> impl Iterator<Item = &'a cookie_store::Cookie<'static>> + 'a {
        self.0.iter_any()
    }

    /// Serialize any __unexpired__ and __persistent__ cookies in the store with `cookie_to_string`
    /// and write them to `writer`
    pub fn save<W, E, F>(&self, writer: &mut W, cookie_to_string: F) -> Result<(), cookie_store::Error>
    where
        W: std::io::Write,
        F: Fn(&cookie_store::Cookie<'static>) -> Result<String, E>,
        cookie_store::Error: From<E>,
    {
        self.0.save(writer, cookie_to_string)
    }

    /// Serialize any **unexpired** and **persistent** cookies in the store to JSON format and write them to `writer`
    pub fn save_json<W: std::io::Write>(&self, writer: &mut W) -> Result<(), cookie_store::Error> {
        self.0.save_json(writer)
    }

    /// Load cookies from `reader`, deserializing with `cookie_from_str`, skipping any **expired** cookies.
    /// The `CookieStore` will be fully reloaded and any stored cookie will be deleted.
    pub fn load<R, E, F>(
        &mut self,
        reader: R,
        cookie_from_str: F
    ) -> Result<(), cookie_store::Error> where
        R: std::io::BufRead,
        F: Fn(&str) -> Result<cookie_store::Cookie<'static>, E>,
        cookie_store::Error: From<E>,
    {
        let cookies = cookie_store::CookieStore::load(reader, cookie_from_str)?;
        self.0 = cookies;
        Ok(())
    }

    /// Load JSON-formatted cookies from `reader`, skipping any **expired** cookies
    /// The `CookieStore` will be fully reloaded and any stored cookie will be deleted.
    pub fn load_json<R: std::io::BufRead>(&mut self, reader: R) -> Result<(), cookie_store::Error> {
        let cookies = cookie_store::CookieStore::load_json(reader)?;
        self.0 = cookies;
        Ok(())
    }    
}

impl<'a> fmt::Debug for CookieStore {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Error representing a parse failure of a 'Set-Cookie' header.
pub(crate) struct CookieParseError(cookie_crate::ParseError);

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
