use std::fmt;
use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use hyper::client::IntoUrl;
use hyper::header::{Headers, ContentType, Location, Referer, UserAgent, Accept, ContentEncoding, Encoding, ContentLength};
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::version::HttpVersion;
use hyper::{Url};

use serde::{Deserialize, Serialize};
use serde_json;
use serde_urlencoded;

use ::body::{self, Body};
use ::redirect::{RedirectPolicy, check_redirect};

static DEFAULT_USER_AGENT: &'static str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// A `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and reuse it.
pub struct Client {
    inner: Arc<ClientRef>,  //::hyper::Client,
}

impl Client {
    /// Constructs a new `Client`.
    pub fn new() -> ::Result<Client> {
        let mut client = try!(new_hyper_client());
        client.set_redirect_policy(::hyper::client::RedirectPolicy::FollowNone);
        Ok(Client {
            inner: Arc::new(ClientRef {
                hyper: client,
                redirect_policy: Mutex::new(RedirectPolicy::default()),
                auto_ungzip: AtomicBool::new(true),
            }),
        })
    }

    /// Enable auto gzip decompression by checking the ContentEncoding response header.
    pub fn gzip(&mut self, enable: bool) {
        self.inner.auto_ungzip.store(enable, Ordering::Relaxed);
    }

    /// Set a `RedirectPolicy` for this client.
    pub fn redirect(&mut self, policy: RedirectPolicy) {
        *self.inner.redirect_policy.lock().unwrap() = policy;
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
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Client")
            .field("redirect_policy", &self.inner.redirect_policy)
            .field("auto_ungzip", &self.inner.auto_ungzip)
            .finish()
    }
}

struct ClientRef {
    hyper: ::hyper::Client,
    redirect_policy: Mutex<RedirectPolicy>,
    auto_ungzip: AtomicBool,
}

fn new_hyper_client() -> ::Result<::hyper::Client> {
    use hyper_native_tls::NativeTlsClient;
    Ok(::hyper::Client::with_connector(
        ::hyper::client::Pool::with_connector(
            Default::default(),
            ::hyper::net::HttpsConnector::new(
                try!(NativeTlsClient::new()
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

impl RequestBuilder {
    /// Add a `Header` to this Request.
    ///
    /// ```no_run
    /// use reqwest::header::UserAgent;
    /// let client = reqwest::Client::new().expect("client failed to construct");
    ///
    /// let res = client.get("https://www.rust-lang.org")
    ///     .header(UserAgent("foo".to_string()))
    ///     .send();
    /// ```
    pub fn header<H: ::header::Header + ::header::HeaderFormat>(mut self, header: H) -> RequestBuilder {
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
    /// ```no_run
    /// # use std::collections::HashMap;
    /// let mut params = HashMap::new();
    /// params.insert("lang", "rust");
    ///
    /// let client = reqwest::Client::new().unwrap();
    /// let res = client.post("http://httpbin.org")
    ///     .form(&params)
    ///     .send();
    /// ```
    pub fn form<T: Serialize>(mut self, form: &T) -> RequestBuilder {
        let body = serde_urlencoded::to_string(form).map_err(::Error::from);
        self.headers.set(ContentType::form_url_encoded());
        self.body = Some(body.map(|b| b.into()));
        self
    }

    /// Send a JSON body.
    ///
    /// Sets the body to the JSON serialization of the passed value, and
    /// also sets the `Content-Type: application/json` header.
    ///
    /// ```no_run
    /// # use std::collections::HashMap;
    /// let mut map = HashMap::new();
    /// map.insert("lang", "rust");
    ///
    /// let client = reqwest::Client::new().unwrap();
    /// let res = client.post("http://httpbin.org")
    ///     .json(&map)
    ///     .send();
    /// ```
    pub fn json<T: Serialize>(mut self, json: &T) -> RequestBuilder {
        let body = serde_json::to_vec(json).expect("serde to_vec cannot fail");
        self.headers.set(ContentType::json());
        self.body = Some(Ok(body.into()));
        self
    }

    /// Constructs the Request and sends it the target URL, returning a Response.
    pub fn send(mut self) -> ::Result<Response> {
        if !self.headers.has::<UserAgent>() {
            self.headers.set(UserAgent(DEFAULT_USER_AGENT.to_owned()));
        }

        if !self.headers.has::<Accept>() {
            self.headers.set(Accept::star());
        }

        let client = self.client;
        let mut method = self.method;
        let mut url = try!(self.url);
        let mut headers = self.headers;
        let mut body = match self.body {
            Some(b) => Some(try!(b)),
            None => None,
        };

        let mut urls = Vec::new();

        loop {
            let res = {
                debug!("request {:?} \"{}\"", method, url);
                let mut req = client.hyper.request(method.clone(), url.clone())
                    .headers(headers.clone());

                if let Some(ref mut b) = body {
                    let body = body::as_hyper_body(b);
                    req = req.body(body);
                }

                try!(req.send())
            };

            let should_redirect = match res.status {
                StatusCode::MovedPermanently |
                StatusCode::Found |
                StatusCode::SeeOther => {
                    body = None;
                    match method {
                        Method::Get | Method::Head => {},
                        _ => {
                            method = Method::Get;
                        }
                    }
                    true
                },
                StatusCode::TemporaryRedirect |
                StatusCode::PermanentRedirect => {
                    if let Some(ref body) = body {
                        body::can_reset(body)
                    } else {
                        true
                    }
                },
                _ => false,
            };

            if should_redirect {
                let loc = {
                    let loc = res.headers.get::<Location>().map(|loc| url.join(loc));
                    if let Some(loc) = loc {
                        loc
                    } else {
                        return Ok(Response {
                            inner: Decoder::from_hyper_response(res, client.auto_ungzip.load(Ordering::Relaxed))
                        });
                    }
                };

                url = match loc {
                    Ok(loc) => {
                        headers.set(Referer(url.to_string()));
                        urls.push(url);
                        if check_redirect(&client.redirect_policy.lock().unwrap(), &loc, &urls)? {
                            loc
                        } else {
                            debug!("redirect_policy disallowed redirection to '{}'", loc);
                            return Ok(Response {
                                inner: Decoder::from_hyper_response(res, client.auto_ungzip.load(Ordering::Relaxed))
                            })
                        }
                    },
                    Err(e) => {
                        debug!("Location header had invalid URI: {:?}", e);
                        return Ok(Response {
                            inner: Decoder::from_hyper_response(res, client.auto_ungzip.load(Ordering::Relaxed))
                        })
                    }
                };

                debug!("redirecting to {:?} '{}'", method, url);

                //TODO: removeSensitiveHeaders(&mut headers, &url);
            } else {
                return Ok(Response {
                    inner: Decoder::from_hyper_response(res, client.auto_ungzip.load(Ordering::Relaxed))
                });
            }
        }
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

/// A Response to a submitted `Request`.
pub struct Response {
    inner: Decoder,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        return match &self.inner {
            &Decoder::PlainText(ref hyper_response) => {
                f.debug_struct("Response")
                    .field("status", &hyper_response.status)
                    .field("headers", &hyper_response.headers)
                    .field("version", &hyper_response.version)
                    .finish()
            },
            &Decoder::Gzip{ref status, ref version, ref headers, ..} => {
                f.debug_struct("Response")
                    .field("status", &status)
                    .field("headers", &headers)
                    .field("version", &version)
                    .finish()
            }
        }
    }
}

impl Response {
    /// Get the `StatusCode`.
    #[inline]
    pub fn status(&self) -> &StatusCode {
        match &self.inner {
            &Decoder::PlainText(ref hyper_response) => &hyper_response.status,
            &Decoder::Gzip{ref status, ..} => status
        }
    }

    /// Get the `Headers`.
    #[inline]
    pub fn headers(&self) -> &Headers {
        match &self.inner {
            &Decoder::PlainText(ref hyper_response) => &hyper_response.headers,
            &Decoder::Gzip{ref headers, ..} => headers
        }
    }

    /// Get the `HttpVersion`.
    #[inline]
    pub fn version(&self) -> &HttpVersion {
        match &self.inner {
            &Decoder::PlainText(ref hyper_response) => &hyper_response.version,
            &Decoder::Gzip{ref version, ..} => version
        }
    }

    /// Try and deserialize the response body as JSON.
    #[inline]
    pub fn json<T: Deserialize>(&mut self) -> ::Result<T> {
        serde_json::from_reader(self).map_err(::Error::from)
    }
}

enum Decoder {
    /// A `PlainText` decoder just returns the response content as is.
    PlainText(::hyper::client::Response),
    /// A `Gzip` decoder will uncompress the gziped response content before returning it.
    Gzip {
        decoder: ::libflate::gzip::Decoder<::hyper::client::Response>,
        headers: ::hyper::header::Headers,
        version: ::hyper::version::HttpVersion,
        status: ::hyper::status::StatusCode,
    }
}

impl Decoder {
    /// Constructs a Decoder from a hyper request.
    ///
    /// A decoder is just a wrapper around the hyper request that knows
    /// how to decode the content body of the request.
    ///
    /// Uses the correct variant by inspecting the Content-Encoding header.
    fn from_hyper_response(res: ::hyper::client::Response, check_gzip: bool) -> Self {
        if !check_gzip {
            return Decoder::PlainText(res);
        }

        let mut is_gzip = false;
        match res.headers.get::<ContentEncoding>() {
            Some(encoding_types) => {
                if encoding_types.contains(&Encoding::Gzip) {
                    is_gzip = true;
                }
                if let Some(content_length) =  res.headers.get::<ContentLength>() {
                    if content_length.0 == 0 {
                        warn!("GZipped response with content-length of 0");
                        is_gzip = false;
                    }
                }
            }
            _ => {}
        }

        if is_gzip {
            return Decoder::Gzip {
                status: res.status.clone(),
                version: res.version.clone(),
                headers: res.headers.clone(),
                decoder: ::libflate::gzip::Decoder::new(res).unwrap(),
            };
        } else {
            return Decoder::PlainText(res);
        }
    }
}

impl Read for Decoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            &mut Decoder::PlainText(ref mut hyper_response) => {
                hyper_response.read(buf)
            },
            &mut Decoder::Gzip{ref mut decoder, ..} => {
                decoder.read(buf)
            }
        }
    }
}

/// Read the body of the Response.
impl Read for Response {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::body;
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
    fn basic_delete_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com";
        let r = client.delete(some_url);

        assert_eq!(r.method, Method::Delete);
        assert_eq!(r.url, Url::parse(some_url));
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
        assert_eq!(r.headers.get::<ContentType>(), Some(&ContentType::form_url_encoded()));

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
