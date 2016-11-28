

use ::body::{self, Body};
use hyper::Url;

use hyper::client::{IntoUrl, RequestBuilder as HyperRequestBuilder};
use hyper::header::{Headers, ContentType, Location, Referer, UserAgent};
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::version::HttpVersion;

use serde::{Deserialize, Serialize};
use serde_json;
use serde_urlencoded;
use std::io::{self, Read};

static DEFAULT_USER_AGENT: &'static str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// A `Client` to make Requests with.
///
/// The Client has various configuration values to tweak, but the defaults
/// are set to what is usually the most commonly desired value.
///
/// The `Client` holds a connection pool internally, so it is advised that
/// you create one and reuse it.
#[derive(Debug)]
pub struct Client {
    inner: ::hyper::Client,
}

impl Client {
    /// Constructs a new `Client`.
    pub fn new() -> ::Result<Client> {
        let mut client = try!(new_hyper_client());
        client.set_redirect_policy(::hyper::client::RedirectPolicy::FollowNone);
        Ok(Client { inner: client })
    }

    /// Convenience method to make a `GET` request to a URL.
    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Get, url)
    }

    /// Convenience method to make a `POST` request to a URL.
    pub fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::Post, url)
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
            client: self,
            method: method,
            url: url,
            _version: HttpVersion::Http11,
            headers: Headers::new(),

            body: None,
        }
    }
}

fn new_hyper_client() -> ::Result<::hyper::Client> {
    use tls::TlsClient;
    Ok(::hyper::Client::with_connector(
        ::hyper::client::Pool::with_connector(
            Default::default(),
            ::hyper::net::HttpsConnector::new(try!(TlsClient::new()))
        )
    ))
}


/// A builder to construct the properties of a `Request`.
#[derive(Debug)]
pub struct RequestBuilder<'a> {
    client: &'a Client,

    method: Method,
    url: Result<Url, ::UrlError>,
    _version: HttpVersion,
    headers: Headers,

    body: Option<::Result<Body>>,
}

impl<'a> RequestBuilder<'a> {
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
    pub fn header<H: ::header::Header + ::header::HeaderFormat>(mut self,
                                                                header: H)
                                                                -> RequestBuilder<'a> {
        self.headers.set(header);
        self
    }
    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    pub fn headers(mut self, headers: ::header::Headers) -> RequestBuilder<'a> {
        self.headers.extend(headers.iter());
        self
    }

    /// Set the request body.
    pub fn body<T: Into<Body>>(mut self, body: T) -> RequestBuilder<'a> {
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
    pub fn form<T: Serialize>(mut self, form: &T) -> RequestBuilder<'a> {
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
    pub fn json<T: Serialize>(mut self, json: &T) -> RequestBuilder<'a> {
        let body = serde_json::to_vec(json).expect("serde to_vec cannot fail");
        self.headers.set(ContentType::json());
        self.body = Some(Ok(body.into()));
        self
    }

    /// Do all finalisation necessary to create a `hyper::Request`. This
    /// includes adding the UserAgent, checking the URL parsed correctly, etc.
    fn verify(&self) -> ::Result<HyperRequestBuilder> {
        // make sure UserAgent is set
        if !self.headers.has::<UserAgent>() {
            self.headers.set(UserAgent(DEFAULT_USER_AGENT.to_owned()));
        }

        // then check for a valid url
        let url = try!(self.url);

        // try to unwrap the result inside the body. This might have failed if
        // the parsing to json/form failed.
        let body = match self.body {
            Some(b) => Some(try!(b)),
            None => None,
        };

        // then convert the body to a hyper body
        let body = body.map(|b| body::as_hyper_body(b));

        let request = self.client.inner.request(self.method, url).headers(self.headers);

        if let Some(b) = body {
            request.body(b);
        }

        Ok(request)
    }

    // - Set UserAgent if not already done
    // - Unwrap url and body
    // - While True:
    //   - Send request
    //   - match on status code
    //     - if redirect:
    //       - update redirect counter (return error if over 10)
    //       - set Referer
    //       - find next location
    //       - deal with invalid location url
    //       - set new url and go to start of loop
    //     - otherwise return the response
    pub fn send(&mut self) -> ::Result<Response> {
        let mut request: HyperRequestBuilder = self.verify()?;
        let redirect_count = 0;

        while redirect_count < 10 {
            debug!("request {:?} \"{}\"",
                   self.method,
                   self.url.expect("This should have already been verified"));

            // try to send the response, if successful, turn it into our Response
            let response: Response = request.send()?.into();

            if let Some(redirect) = response.parse_redirect(request) {
                // If we got a valid redirect, increment the count and then
                // use the new request builder.
                redirect_count += 1;
                request = redirect.verify()?;
            } else {
                return Ok(response);
            }
        }

        // if we get here it means we've been redirected too many times
        Err(::Error::TooManyRedirects)
    }
}

/// A Response to a submitted `Request`.
#[derive(Debug)]
pub struct Response {
    inner: ::hyper::client::Response,
}

impl Response {
    /// Get the `StatusCode`.
    pub fn status(&self) -> &StatusCode {
        &self.inner.status
    }

    /// Get the `Headers`.
    pub fn headers(&self) -> &Headers {
        &self.inner.headers
    }

    /// Get the `HttpVersion`.
    pub fn version(&self) -> &HttpVersion {
        &self.inner.version
    }

    /// Try and deserialize the response body as JSON.
    pub fn json<T: Deserialize>(&mut self) -> ::Result<T> {
        serde_json::from_reader(self).map_err(::Error::from)
    }

    /// Check if the response is a redirect.
    pub fn is_redirect(&self) -> bool {
        match *self.status() {
            StatusCode::MovedPermanently |
            StatusCode::Found |
            StatusCode::SeeOther => true,
            _ => false,
        }
    }

    /// If the response is a redirect, create a new Request which will go to the
    /// next link.
    fn parse_redirect(&self, from: RequestBuilder) -> Option<RequestBuilder> {
        // turn Post/Put requests into Get
        // Get the Location header
        // join it with the current url if Location exists
        // If the result is None, then return the response
        // otherwise follow the redirect

        if !self.is_redirect() {
            return None;
        }

        // Copy the original request so we keep all the headers, body, etc
        let mut new_request = from.clone();

        // Convert any Post or Put requests to a Get
        new_request.method = match from.method {
            Method::Post | Method::Put => Method::Get,
            m => m,
        };

        // Make sure to set the referer
        let prev_url = from.url.expect("This should have already been verified");
        new_request.header(Referer(prev_url.into_string()));

        // find where we are being redirected to and set the next url
        if let Some(loc) = self.headers().get::<Location>() {
            new_request.url = prev_url.join(loc);
            Some(new_request)
        } else {
            // Location parsing failed, or this isn't a redirect
            None
        }
    }
}


impl From<::hyper::client::Response> for Response {
    fn from(other: ::hyper::client::Response) -> Response {
        Response { inner: other }
    }
}

/// Read the body of the Response.
impl Read for Response {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}


#[cfg(test)]
mod tests {
    use ::body;
    use hyper::Url;
    use hyper::header::{Host, Headers, ContentType};
    use hyper::method::Method;
    use serde_json;
    use serde_urlencoded;
    use std::collections::HashMap;
    use super::*;

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

    #[test]
    fn check_redirect_works() {
        // we know going to 'http://google.com/' should redirect to https,
        // so we use that to see if redirects work properly.
        // TODO: Delete this method when done. It makes a network call!
        let client = Client::new().unwrap();
        let some_url = Url::parse("http://google.com/").unwrap();
        let response = client.get(some_url.clone()).send().unwrap();

        // Check for a redirect indirectly
        assert!(response.inner.url != some_url);
    }

    #[test]
    fn response_is_redirect() {}
}
