//! HTTP Cookies

use std::convert::TryInto;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use crate::async_impl::body::ResponseBody;
use crate::error::Kind;
use crate::header::{HeaderValue, SET_COOKIE};
use crate::{Body, Error};
use bytes::Bytes;
use http::{HeaderMap, Request, Response};
use tower::Service;
use url::Url;

/// Actions for a persistent cookie store providing session support.
pub trait CookieStore: Send + Sync {
    /// Store a set of Set-Cookie header values received from `url`
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url);
    /// Get any Cookie values in the store for `url`
    fn cookies(&self, url: &url::Url) -> Option<HeaderValue>;
}

/// A single HTTP cookie.
pub struct Cookie<'a>(cookie_crate::Cookie<'a>);

/// A good default `CookieStore` implementation.
///
/// This is the implementation used when simply calling `cookie_store(true)`.
/// This type is exposed to allow creating one and filling it with some
/// existing cookies more easily, before creating a `Client`.
///
/// For more advanced scenarios, such as needing to serialize the store or
/// manipulate it between requests, you may refer to the
/// [reqwest_cookie_store crate](https://crates.io/crates/reqwest_cookie_store).
#[derive(Debug, Default)]
pub struct Jar(RwLock<cookie_store::CookieStore>);
impl Clone for Jar{
    fn clone(&self) -> Self {
        Self(RwLock::new(self.0.write().unwrap().clone()))
    }
}
// ===== impl Cookie =====

impl<'a> Cookie<'a> {
    fn parse(value: &'a HeaderValue) -> Result<Cookie<'a>, CookieParseError> {
        std::str::from_utf8(value.as_bytes())
            .map_err(cookie_crate::ParseError::from)
            .and_then(cookie_crate::Cookie::parse)
            .map_err(CookieParseError)
            .map(Cookie)
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
        self.0.max_age().map(|d| {
            d.try_into()
                .expect("time::Duration into std::time::Duration")
        })
    }

    /// The cookie expiration time.
    pub fn expires(&self) -> Option<SystemTime> {
        match self.0.expires() {
            Some(cookie_crate::Expiration::DateTime(offset)) => Some(SystemTime::from(offset)),
            None | Some(cookie_crate::Expiration::Session) => None,
        }
    }
}

impl<'a> fmt::Debug for Cookie<'a> {
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

// ===== impl Jar =====

impl Jar {
    /// Add a cookie to this jar.
    ///
    /// # Example
    ///
    /// ```
    /// use reqwest::{cookie::Jar, Url};
    ///
    /// let cookie = "foo=bar; Domain=yolo.local";
    /// let url = "https://yolo.local".parse::<Url>().unwrap();
    ///
    /// let jar = Jar::default();
    /// jar.add_cookie_str(cookie, &url);
    ///
    /// // and now add to a `ClientBuilder`?
    /// ```
    pub fn add_cookie_str(&self, cookie: &str, url: &url::Url) {
        let cookies = cookie_crate::Cookie::parse(cookie)
            .ok()
            .map(|c| c.into_owned())
            .into_iter();
        self.0.write().unwrap().store_response_cookies(cookies, url);
    }
    /// private
    async fn extract_response_cookie_headers<'a>(
        &self,
        headers: &'a hyper::HeaderMap,
    ) -> impl Iterator<Item = &'a HeaderValue> + 'a {
        headers.get_all(SET_COOKIE).iter()
    }
    /// extract response cookie
    pub (crate)async fn extract_response_cookies<'a>(
        headers: &'a hyper::HeaderMap,
    ) -> impl Iterator<Item = Result<Cookie<'a>, CookieParseError>> + 'a {
        headers
            .get_all(SET_COOKIE)
            .iter()
            .map(|value| Cookie::parse(value))
    }

    /// set cookies from response
    async fn set_cookies_from_response_headers(
        &self,
        headers: &HeaderMap<HeaderValue>,
        url: &url::Url,
    ) -> crate::Result<()> {
        let mut cookies = self
            .extract_response_cookie_headers(headers)
            .await
            .peekable();
        if cookies.peek().is_some() {
            self.set_cookies(&mut cookies, &url);
        }
        Ok(())
    }
}

impl CookieStore for Jar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let iter =
            cookie_headers.filter_map(|val| Cookie::parse(val).map(|c| c.0.into_owned()).ok());

        self.0.write().unwrap().store_response_cookies(iter, url);
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let s = self
            .0
            .read()
            .unwrap()
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");

        if s.is_empty() {
            return None;
        }

        HeaderValue::from_maybe_shared(Bytes::from(s)).ok()
    }
}
/// a service enables an async client or h3 client to manage cookies
#[derive(Debug)]
pub struct CookiesEnabledService<S>
where
    S: Service<Request<Body>, Response = http::Response<ResponseBody>, Error = crate::error::Error>+Clone,
    S::Future: Sync + Send + 'static,
{
    store: Arc<Jar>,
    inner_service: S,
}
impl<
        S: Service<
            Request<Body>,
            Response = http::Response<ResponseBody>,
            Error = crate::error::Error,
            Future: Sync + Send + 'static,
        >+Clone,
    > CookiesEnabledService<S>
{
    /// create a CookieService
    pub fn new(service: S, cookie_store: Arc<Jar>) -> Self {
        Self {
            store: cookie_store,
            inner_service: service,
        }
    }
}
impl<
        S: Service<
            Request<Body>,
            Response = http::Response<ResponseBody>,
            Error = crate::error::Error,
            Future: Sync + Send + 'static,
        >+Clone,
    > Service<Request<Body>> for CookiesEnabledService<S>
{
    type Response = Response<ResponseBody>;

    type Error = Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + Sync>>;

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        // check and add cookies to the request header
        let url = Url::parse(req.uri().to_string().as_str()).expect("invalid URL");
        let headers = req.headers_mut();
        crate::util::add_cookie_header(headers, self.store.as_ref(), &url);
        // call the inner service's call fn
        let inner_response_future = self.inner_service.call(req);

        let store = self.store.clone();
        // store the cookies from response
        Box::pin(async move {
            let response = inner_response_future.await;
            if let Ok(res) = response {
                store
                    .set_cookies_from_response_headers(res.headers(), &url)
                    .await
                    .expect("error set cookies from response");
                return Ok(res);
            }
            Err(Error::new(
                Kind::Body,
                Some("error extract response in cookie service"),
            ))
        })
    }

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner_service.poll_ready(cx)
    }
}
impl<
        S: Service<
            Request<Body>,
            Response = http::Response<ResponseBody>,
            Error = crate::error::Error,
            Future: Sync + Send + 'static,
        >+Clone,
    > Clone for CookiesEnabledService<S>
{
    fn clone(&self) -> Self {
        Self { store: Arc::new(self.store.as_ref().clone()), inner_service: self.inner_service.clone() }
    }
}
