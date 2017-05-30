use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::fs;
use std::io::Read;

use hyper::client::IntoUrl;
use hyper::header::{Headers, ContentType, Location, Referer, UserAgent, Accept, Encoding,
                    AcceptEncoding, Range, qitem};
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::version::HttpVersion;
use hyper::Url;
use hyper::mime;

use serde::Serialize;
use serde_json;
use serde_urlencoded;

use uuid::Uuid;

use body::{self, Body};
use redirect::{self, RedirectPolicy, check_redirect, remove_sensitive_headers};
use response::Response;
use file::File;

static DEFAULT_USER_AGENT: &'static str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub type Params<'a> = Vec<(&'a str, &'a str)>;

macro_rules! write_bytes {
    ($buf:ident, $f:expr, $a:expr) => (
        $buf.extend(format!($f, $a).as_bytes())
    )
}

macro_rules! impl_send {
    ($sname:ident) => (
        impl $sname {
            /// Constructs the Request and sends it the target URL, returning a Response.
            pub fn send(mut self) -> ::Result<Response> {
                if !self.headers.has::<UserAgent>() {
                    self.headers
                        .set(UserAgent(DEFAULT_USER_AGENT.to_owned()));
                }

                if !self.headers.has::<Accept>() {
                    self.headers.set(Accept::star());
                }
                if self.client.auto_ungzip.load(Ordering::Relaxed) &&
                   !self.headers.has::<AcceptEncoding>() && !self.headers.has::<Range>() {
                    self.headers
                        .set(AcceptEncoding(vec![qitem(Encoding::Gzip)]));
                }
                let client = self.client;
                let mut method = self.method;
                let mut url = try_!(self.url);
                let mut headers = self.headers;
                let mut body = match self.body {
                    Some(b) => Some(try_!(b)),
                    None => None,
                };

                let mut urls = Vec::new();

                loop {
                    let res = {
                        info!("Request: {:?} {}", method, url);
                        let c = client.hyper.read().unwrap();
                        let mut req = c.request(method.clone(), url.clone())
                            .headers(headers.clone());

                        if let Some(ref mut b) = body {
                            let body = body::as_hyper_body(b);
                            req = req.body(body);
                        }

                        try_!(req.send(), &url)
                    };

                    let should_redirect = match res.status {
                        StatusCode::MovedPermanently |
                        StatusCode::Found |
                        StatusCode::SeeOther => {
                            body = None;
                            match method {
                                Method::Get | Method::Head => {}
                                _ => {
                                    method = Method::Get;
                                }
                            }
                            true
                        }
                        StatusCode::TemporaryRedirect |
                        StatusCode::PermanentRedirect => {
                            if let Some(ref body) = body {
                                body::can_reset(body)
                            } else {
                                true
                            }
                        }
                        _ => false,
                    };

                    if should_redirect {
                        let loc = {
                            let loc = res.headers.get::<Location>().map(|loc| url.join(loc));
                            if let Some(loc) = loc {
                                loc
                            } else {
                                return Ok(::response::new(res, client.auto_ungzip.load(Ordering::Relaxed)));
                            }
                        };

                        url = match loc {
                            Ok(loc) => {
                                if client.auto_referer.load(Ordering::Relaxed) {
                                    if let Some(referer) = make_referer(&loc, &url) {
                                        headers.set(referer);
                                    }
                                }
                                urls.push(url);
                                let action =
                                    check_redirect(&client.redirect_policy.lock().unwrap(), &loc, &urls);

                                match action {
                                    redirect::Action::Follow => loc,
                                    redirect::Action::Stop => {
                                        debug!("redirect_policy disallowed redirection to '{}'", loc);
                                        return Ok(::response::new(res,
                                                                  client
                                                                      .auto_ungzip
                                                                      .load(Ordering::Relaxed)));
                                    }
                                    redirect::Action::LoopDetected => {
                                        return Err(::error::loop_detected(res.url.clone()));
                                    }
                                    redirect::Action::TooManyRedirects => {
                                        return Err(::error::too_many_redirects(res.url.clone()));
                                    }
                                }
                            }
                            Err(e) => {
                                debug!("Location header had invalid URI: {:?}", e);

                                return Ok(::response::new(res, client.auto_ungzip.load(Ordering::Relaxed)));
                            }
                        };

                        remove_sensitive_headers(&mut headers, &url, &urls);
                        debug!("redirecting to {:?} '{}'", method, url);
                    } else {
                        return Ok(::response::new(res, client.auto_ungzip.load(Ordering::Relaxed)));
                    }
                }
            }
        }
    )
}

/// A `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and reuse it.
///
/// # Examples
///
/// ```rust
/// # use reqwest::{Error, Client};
/// #
/// # fn run() -> Result<(), Error> {
/// let client = Client::new()?;
/// let resp = client.get("http://httpbin.org/").send()?;
/// #   drop(resp);
/// #   Ok(())
/// # }
///
/// ```
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientRef>,
}

impl Client {
    /// Constructs a new `Client`.
    pub fn new() -> ::Result<Client> {
        let mut client = try_!(new_hyper_client());
        client.set_redirect_policy(::hyper::client::RedirectPolicy::FollowNone);
        Ok(Client {
               inner: Arc::new(ClientRef {
                                   hyper: RwLock::new(client),
                                   redirect_policy: Mutex::new(RedirectPolicy::default()),
                                   auto_referer: AtomicBool::new(true),
                                   auto_ungzip: AtomicBool::new(true),
                               }),
           })
    }

    /// Enable auto gzip decompression by checking the ContentEncoding response header.
    ///
    /// Default is enabled.
    pub fn gzip(&mut self, enable: bool) {
        self.inner.auto_ungzip.store(enable, Ordering::Relaxed);
    }

    /// Set a `RedirectPolicy` for this client.
    ///
    /// Default will follow redirects up to a maximum of 10.
    pub fn redirect(&mut self, policy: RedirectPolicy) {
        *self.inner.redirect_policy.lock().unwrap() = policy;
    }

    /// Enable or disable automatic setting of the `Referer` header.
    ///
    /// Default is `true`.
    pub fn referer(&mut self, enable: bool) {
        self.inner.auto_referer.store(enable, Ordering::Relaxed);
    }

    /// Set a timeout for both the read and write operations of a client.
    pub fn timeout(&mut self, timeout: Duration) {
        let mut client = self.inner.hyper.write().unwrap();
        client.set_read_timeout(Some(timeout));
        client.set_write_timeout(Some(timeout));
    }

    /// Convenience method to make a `GET` request to a URL.
    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Get, url)
    }

    /// Convenience method to make a `POST` request to a URL.
    pub fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Post, url)
    }

    /// Convenience method to make a `PUT` request to a URL.
    pub fn put<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Put, url)
    }

    /// Convenience method to make a `PATCH` request to a URL.
    pub fn patch<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Patch, url)
    }

    /// Convenience method to make a `DELETE` request to a URL.
    pub fn delete<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Delete, url)
    }

    /// Convenience method to make a `HEAD` request to a URL.
    pub fn head<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Head, url)
    }

    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// request body before sending.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        let url = url.into_url();
        RequestBuilder {
            client: self.inner.clone(),
            method: method,
            url: url,
            _version: HttpVersion::Http11,
            headers: Headers::new(),

            body: None,
        }
    }

    /// Start building a `multipart/form-data POST Request` with the `Url`.
    ///
    /// Returns a `MultipartRequestBuilder`.
    pub fn multipart<'a, U: IntoUrl>(&self,
                                     url: U,
                                     files: Vec<File>,
                                     params: Params<'a>)
                                     -> ::Result<MultipartRequestBuilder> {
        let mut body: Vec<u8> = Vec::new();
        let boundary = MultipartRequestBuilder::choose_boundary();
        let multipart_mime = ContentType(format!{"multipart/form-data; boundary={}", boundary}
                                             .parse::<mime::Mime>()
                                             .unwrap());

        for (name, value) in params {
            write_bytes!(body, "\r\n--{}\r\n", boundary);
            write_bytes!(body, "Content-Disposition: form-data; name\"{}\"", name);
            write_bytes!(body, "\r\n{}\r\n", value);
        }

        for File { name, path, mime } in files {
            write_bytes!(body, "\r\n--{}\r\n", boundary);
            write_bytes!(body, "Content-Disposition: form-data; name\"{}\"", name);
            write_bytes!(body,
                         "; filename=\"{}\"",
                         path.file_name().unwrap().to_str().unwrap());
            write_bytes!(body, "\r\nContent-type: {}\r\n\r\n", mime.unwrap());
            let mut content = try_!(fs::File::open(path));
            content.read_to_end(&mut body).unwrap();
            body.extend("\r\n\r\n".as_bytes());
        }

        write_bytes!(body, "\r\n--{}--", boundary);
        let mut req = self.request(Method::Post, url).body(body);
        req.headers.set(multipart_mime);

        Ok(MultipartRequestBuilder { request: req })
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
            .field("redirect_policy", &self.inner.redirect_policy)
            .field("referer", &self.inner.auto_referer)
            .field("auto_ungzip", &self.inner.auto_ungzip)
            .finish()
    }
}

struct ClientRef {
    hyper: RwLock<::hyper::Client>,
    redirect_policy: Mutex<RedirectPolicy>,
    auto_referer: AtomicBool,
    auto_ungzip: AtomicBool,
}

fn new_hyper_client() -> ::Result<::hyper::Client> {
    use hyper_native_tls::NativeTlsClient;
    Ok(::hyper::Client::with_connector(
        ::hyper::client::Pool::with_connector(
            Default::default(),
            ::hyper::net::HttpsConnector::new(
                try_!(NativeTlsClient::new()
                     .map_err(|e| ::hyper::Error::Ssl(Box::new(e)))))
        )
    ))
}

/// A builder to construct the properties of a `Request`.
pub struct RequestBuilder {
    client: Arc<ClientRef>,

    method: Method,
    url: Result<Url, ::UrlError>,
    _version: HttpVersion,
    headers: Headers,

    body: Option<::Result<Body>>,
}

impl_send!(RequestBuilder);

impl RequestBuilder {
    /// Add a `Header` to this Request.
    ///
    /// ```rust
    /// # use reqwest::Error;
    /// #
    /// # fn run() -> Result<(), Error> {
    /// use reqwest::header::UserAgent;
    /// let client = reqwest::Client::new()?;
    ///
    /// let res = client.get("https://www.rust-lang.org")
    ///     .header(UserAgent("foo".to_string()))
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn header<H: ::header::Header + ::header::HeaderFormat>(mut self,
                                                                header: H)
                                                                -> RequestBuilder {
        self.headers.set(header);
        self
    }
    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    pub fn headers(mut self, headers: ::header::Headers) -> RequestBuilder {
        self.headers.extend(headers.iter());
        self
    }

    /// Enable HTTP basic authentication.
    pub fn basic_auth<U, P>(self, username: U, password: Option<P>) -> RequestBuilder
        where U: Into<String>,
              P: Into<String>
    {
        self.header(::header::Authorization(::header::Basic {
                                                username: username.into(),
                                                password: password.map(|p| p.into()),
                                            }))
    }

    /// Set the request body.
    pub fn body<T: Into<Body>>(mut self, body: T) -> RequestBuilder {
        self.body = Some(Ok(body.into()));
        self
    }

    /// Send a form body.
    ///
    /// Sets the body to the url encoded serialization of the passed value,
    /// and also sets the `Content-Type: application/www-form-url-encoded`
    /// header.
    ///
    /// ```rust
    /// # use reqwest::Error;
    /// # use std::collections::HashMap;
    /// #
    /// # fn run() -> Result<(), Error> {
    /// let mut params = HashMap::new();
    /// params.insert("lang", "rust");
    ///
    /// let client = reqwest::Client::new()?;
    /// let res = client.post("http://httpbin.org")
    ///     .form(&params)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn form<T: Serialize>(mut self, form: &T) -> RequestBuilder {
        let body = serde_urlencoded::to_string(form).map_err(::error::from);
        self.headers.set(ContentType::form_url_encoded());
        self.body = Some(body.map(|b| b.into()));
        self
    }

    /// Send a JSON body.
    ///
    /// Sets the body to the JSON serialization of the passed value, and
    /// also sets the `Content-Type: application/json` header.
    ///
    /// ```rust
    /// # use reqwest::Error;
    /// # use std::collections::HashMap;
    /// #
    /// # fn run() -> Result<(), Error> {
    /// let mut map = HashMap::new();
    /// map.insert("lang", "rust");
    ///
    /// let client = reqwest::Client::new()?;
    /// let res = client.post("http://httpbin.org")
    ///     .json(&map)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn json<T: Serialize>(mut self, json: &T) -> RequestBuilder {
        let body = serde_json::to_vec(json).expect("serde to_vec cannot fail");
        self.headers.set(ContentType::json());
        self.body = Some(Ok(body.into()));
        self
    }
}

impl fmt::Debug for RequestBuilder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RequestBuilder")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &self.headers)
            .finish()
    }
}

pub struct MultipartRequestBuilder {
    request: RequestBuilder,
}

impl MultipartRequestBuilder {
    pub fn send(self) -> ::Result<Response> {
        self.request.send()
    }

    fn choose_boundary() -> String {
        Uuid::new_v4().simple().to_string()
    }
}

fn make_referer(next: &Url, previous: &Url) -> Option<Referer> {
    if next.scheme() == "http" && previous.scheme() == "https" {
        return None;
    }

    let mut referer = previous.clone();
    let _ = referer.set_username("");
    let _ = referer.set_password(None);
    referer.set_fragment(None);
    Some(Referer(referer.into_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    use body;
    use hyper::method::Method;
    use hyper::Url;
    use hyper::header::{Host, Headers, ContentType};
    use std::collections::HashMap;
    use serde_urlencoded;
    use serde_json;

    #[test]
    fn basic_get_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.get(some_url);

        assert_eq!(r.method, Method::Get);
        assert_eq!(r.url, Url::parse(some_url));
    }

    #[test]
    fn basic_head_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.head(some_url);

        assert_eq!(r.method, Method::Head);
        assert_eq!(r.url, Url::parse(some_url));
    }

    #[test]
    fn basic_post_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.post(some_url);

        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url, Url::parse(some_url));
    }

    #[test]
    fn basic_put_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com";
        let r = client.put(some_url);

        assert_eq!(r.method, Method::Put);
        assert_eq!(r.url, Url::parse(some_url));
    }

    #[test]
    fn basic_patch_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com";
        let r = client.patch(some_url);

        assert_eq!(r.method, Method::Patch);
        assert_eq!(r.url, Url::parse(some_url));
    }

    #[test]
    fn basic_delete_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com";
        let r = client.delete(some_url);

        assert_eq!(r.method, Method::Delete);
        assert_eq!(r.url, Url::parse(some_url));
    }

    #[test]
    fn basic_multipart_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com";
        let mime: mime::Mime = "text/plain".parse().unwrap();
        let file = vec![File {
                            name: "tomlfile".to_string(),
                            path: &Path::new("Cargo.toml"),
                            mime: Some(mime),
                        }];
        let r = client
            .multipart(some_url, file, vec![("foo", "bar")])
            .unwrap();

        assert_eq!(r.request.method, Method::Post);
        assert_eq!(r.request.url, Url::parse(some_url));
    }

    #[test]
    fn add_header() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let header = Host {
            hostname: "google.com".to_string(),
            port: None,
        };

        // Add a copy of the header to the request builder
        r = r.header(header.clone());

        // then check it was actually added
        assert_eq!(r.headers.get::<Host>(), Some(&header));
    }

    #[test]
    fn add_headers() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let header = Host {
            hostname: "google.com".to_string(),
            port: None,
        };

        let mut headers = Headers::new();
        headers.set(header);

        // Add a copy of the headers to the request builder
        r = r.headers(headers.clone());

        // then make sure they were added correctly
        assert_eq!(r.headers, headers);
    }

    #[test]
    fn add_body() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let body = "Some interesting content";

        r = r.body(body);

        let buf = body::read_to_string(r.body.unwrap().unwrap()).unwrap();

        assert_eq!(buf, body);
    }

    #[test]
    fn add_form() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let mut form_data = HashMap::new();
        form_data.insert("foo", "bar");

        r = r.form(&form_data);

        // Make sure the content type was set
        assert_eq!(r.headers.get::<ContentType>(),
                   Some(&ContentType::form_url_encoded()));

        let buf = body::read_to_string(r.body.unwrap().unwrap()).unwrap();

        let body_should_be = serde_urlencoded::to_string(&form_data).unwrap();
        assert_eq!(buf, body_should_be);
    }

    #[test]
    fn add_json() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let mut json_data = HashMap::new();
        json_data.insert("foo", "bar");

        r = r.json(&json_data);

        // Make sure the content type was set
        assert_eq!(r.headers.get::<ContentType>(), Some(&ContentType::json()));

        let buf = body::read_to_string(r.body.unwrap().unwrap()).unwrap();

        let body_should_be = serde_json::to_string(&json_data).unwrap();
        assert_eq!(buf, body_should_be);
    }
}
