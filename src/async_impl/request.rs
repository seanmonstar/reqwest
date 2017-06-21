use std::fmt;

use serde::Serialize;
use serde_json;
use serde_urlencoded;

use super::body::{self, Body};
use super::client::{Client, Pending};
use header::{ContentType, Headers};
use {Method, Url};

/// A request which can be executed with `Client::execute()`.
pub struct Request {
    method: Method,
    url: Url,
    headers: Headers,
    body: Option<Body>,
}

/// A builder to construct the properties of a `Request`.
pub struct RequestBuilder {
    client: Client,
    request: Option<Request>,
}

impl Request {
    /// Constructs a new request.
    #[inline]
    pub fn new(method: Method, url: Url) -> Self {
        Request {
            method,
            url,
            headers: Headers::new(),
            body: None,
        }
    }

    /// Get the method.
    #[inline]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Get a mutable reference to the method.
    #[inline]
    pub fn method_mut(&mut self) -> &mut Method {
        &mut self.method
    }

    /// Get the url.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Get a mutable reference to the url.
    #[inline]
    pub fn url_mut(&mut self) -> &mut Url {
        &mut self.url
    }

    /// Get the headers.
    #[inline]
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// Get a mutable reference to the headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut Headers {
        &mut self.headers
    }

    /// Get the body.
    #[inline]
    pub fn body(&self) -> Option<&Body> {
        self.body.as_ref()
    }

    /// Get a mutable reference to the body.
    #[inline]
    pub fn body_mut(&mut self) -> &mut Option<Body> {
        &mut self.body
    }
}

impl RequestBuilder {
    /// Add a `Header` to this Request.
    pub fn header<H>(&mut self, header: H) -> &mut RequestBuilder
    where
        H: ::header::Header,
    {
        self.request_mut().headers.set(header);
        self
    }
    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    pub fn headers(&mut self, headers: ::header::Headers) -> &mut RequestBuilder {
        self.request_mut().headers.extend(headers.iter());
        self
    }

    /// Enable HTTP basic authentication.
    pub fn basic_auth<U, P>(&mut self, username: U, password: Option<P>) -> &mut RequestBuilder
    where
        U: Into<String>,
        P: Into<String>,
    {
        self.header(::header::Authorization(::header::Basic {
            username: username.into(),
            password: password.map(|p| p.into()),
        }))
    }

    /// Set the request body.
    pub fn body<T: Into<Body>>(&mut self, body: T) -> &mut RequestBuilder {
        self.request_mut().body = Some(body.into());
        self
    }

    /// Send a form body.
    pub fn form<T: Serialize>(&mut self, form: &T) -> ::Result<&mut RequestBuilder> {
        {
            // check request_mut() before running serde
            let mut req = self.request_mut();
            let body = try_!(serde_urlencoded::to_string(form));
            req.headers.set(ContentType::form_url_encoded());
            req.body = Some(body::reusable(body.into()));
        }
        Ok(self)
    }

    /// Send a JSON body.
    ///
    /// # Errors
    ///
    /// Serialization can fail if `T`'s implementation of `Serialize` decides to
    /// fail, or if `T` contains a map with non-string keys.
    pub fn json<T: Serialize>(&mut self, json: &T) -> ::Result<&mut RequestBuilder> {
        {
            // check request_mut() before running serde
            let mut req = self.request_mut();
            let body = try_!(serde_json::to_vec(json));
            req.headers.set(ContentType::json());
            req.body = Some(body::reusable(body.into()));
        }
        Ok(self)
    }

    /// Build a `Request`, which can be inspected, modified and executed with
    /// `Client::execute()`.
    ///
    /// # Panics
    ///
    /// This method consumes builder internal state. It panics on an attempt to
    /// reuse already consumed builder.
    pub fn build(&mut self) -> Request {
        self.request
            .take()
            .expect("RequestBuilder cannot be reused after builder a Request")
    }

    /// Constructs the Request and sends it the target URL, returning a Response.
    ///
    /// # Errors
    ///
    /// This method fails if there was an error while sending request,
    /// redirect loop was detected or redirect limit was exhausted.
    pub fn send(&mut self) -> Pending {
        let request = self.build();
        self.client.execute(request)
    }

    // private

    fn request_mut(&mut self) -> &mut Request {
        self.request
            .as_mut()
            .expect("RequestBuilder cannot be reused after builder a Request")
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt_request_fields(&mut f.debug_struct("Request"), self)
            .finish()
    }
}

impl fmt::Debug for RequestBuilder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref req) = self.request {
            fmt_request_fields(&mut f.debug_struct("RequestBuilder"), req)
                .finish()
        } else {
            f.debug_tuple("RequestBuilder")
                .field(&"Consumed")
                .finish()
        }
    }
}

fn fmt_request_fields<'a, 'b>(f: &'a mut fmt::DebugStruct<'a, 'b>, req: &Request) -> &'a mut fmt::DebugStruct<'a, 'b> {
    f.field("method", &req.method)
        .field("url", &req.url)
        .field("headers", &req.headers)
}

// pub(crate)

#[inline]
pub fn builder(client: Client, req: Request) -> RequestBuilder {
    RequestBuilder {
        client: client,
        request: Some(req),
    }
}

#[inline]
pub fn pieces(req: Request) -> (Method, Url, Headers, Option<Body>) {
    (req.method, req.url, req.headers, req.body)
}

#[cfg(test)]
mod tests {
    /*
    use {body, Method};
    use super::Client;
    use header::{Host, Headers, ContentType};
    use std::collections::HashMap;
    use serde_urlencoded;
    use serde_json;

    #[test]
    fn basic_get_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.get(some_url).unwrap().build();

        assert_eq!(r.method, Method::Get);
        assert_eq!(r.url.as_str(), some_url);
    }

    #[test]
    fn basic_head_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.head(some_url).unwrap().build();

        assert_eq!(r.method, Method::Head);
        assert_eq!(r.url.as_str(), some_url);
    }

    #[test]
    fn basic_post_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.post(some_url).unwrap().build();

        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url.as_str(), some_url);
    }

    #[test]
    fn basic_put_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.put(some_url).unwrap().build();

        assert_eq!(r.method, Method::Put);
        assert_eq!(r.url.as_str(), some_url);
    }

    #[test]
    fn basic_patch_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.patch(some_url).unwrap().build();

        assert_eq!(r.method, Method::Patch);
        assert_eq!(r.url.as_str(), some_url);
    }

    #[test]
    fn basic_delete_request() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let r = client.delete(some_url).unwrap().build();

        assert_eq!(r.method, Method::Delete);
        assert_eq!(r.url.as_str(), some_url);
    }

    #[test]
    fn add_header() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url).unwrap();

        let header = Host {
            hostname: "google.com".to_string(),
            port: None,
        };

        // Add a copy of the header to the request builder
        let r = r.header(header.clone()).build();

        // then check it was actually added
        assert_eq!(r.headers.get::<Host>(), Some(&header));
    }

    #[test]
    fn add_headers() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url).unwrap();

        let header = Host {
            hostname: "google.com".to_string(),
            port: None,
        };

        let mut headers = Headers::new();
        headers.set(header);

        // Add a copy of the headers to the request builder
        let r = r.headers(headers.clone()).build();

        // then make sure they were added correctly
        assert_eq!(r.headers, headers);
    }

    #[test]
    fn add_body() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url).unwrap();

        let body = "Some interesting content";

        let r = r.body(body).build();

        let buf = body::read_to_string(r.body.unwrap()).unwrap();

        assert_eq!(buf, body);
    }

    #[test]
    fn add_form() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url).unwrap();

        let mut form_data = HashMap::new();
        form_data.insert("foo", "bar");

        let r = r.form(&form_data).unwrap().build();

        // Make sure the content type was set
        assert_eq!(r.headers.get::<ContentType>(),
                   Some(&ContentType::form_url_encoded()));

        let buf = body::read_to_string(r.body.unwrap()).unwrap();

        let body_should_be = serde_urlencoded::to_string(&form_data).unwrap();
        assert_eq!(buf, body_should_be);
    }

    #[test]
    fn add_json() {
        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url).unwrap();

        let mut json_data = HashMap::new();
        json_data.insert("foo", "bar");

        let r = r.json(&json_data).unwrap().build();

        // Make sure the content type was set
        assert_eq!(r.headers.get::<ContentType>(), Some(&ContentType::json()));

        let buf = body::read_to_string(r.body.unwrap()).unwrap();

        let body_should_be = serde_json::to_string(&json_data).unwrap();
        assert_eq!(buf, body_should_be);
    }

    #[test]
    fn add_json_fail() {
        use serde::{Serialize, Serializer};
        use serde::ser::Error;
        struct MyStruct;
        impl Serialize for MyStruct {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
                where S: Serializer
                {
                    Err(S::Error::custom("nope"))
                }
        }

        let client = Client::new().unwrap();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url).unwrap();
        let json_data = MyStruct{};
        assert!(r.json(&json_data).unwrap_err().is_serialization());
    }
    */
}
