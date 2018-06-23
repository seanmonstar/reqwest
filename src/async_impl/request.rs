use std::fmt;

use base64::{encode};
use mime::{self};
use serde::Serialize;
use serde_json;
use serde_urlencoded;

use super::body::{self, Body};
use super::client::{Client, Pending, pending_err};
use header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use http::HttpTryFrom;
use {Method, Url};

/// A request which can be executed with `Client::execute()`.
pub struct Request {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Body>,
}

/// A builder to construct the properties of a `Request`.
pub struct RequestBuilder {
    client: Client,
    request: Option<Request>,
    err: Option<::Error>,
}

impl Request {
    /// Constructs a new request.
    #[inline]
    pub fn new(method: Method, url: Url) -> Self {
        Request {
            method,
            url,
            headers: HeaderMap::new(),
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
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to the headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
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
    pub fn header<K, V>(&mut self, key: K, value: V) -> &mut RequestBuilder
    where
        HeaderName: HttpTryFrom<K>,
        HeaderValue: HttpTryFrom<V>,
    {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            match <HeaderName as HttpTryFrom<K>>::try_from(key) {
                Ok(key) => {
                    match <HeaderValue as HttpTryFrom<V>>::try_from(value) {
                        Ok(value) => { req.headers_mut().append(key, value); }
                        Err(e) => self.err = Some(::error::from(e.into())),
                    }
                },
                Err(e) => self.err = Some(::error::from(e.into())),
            };
        }
        self
    }
    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    pub fn headers(&mut self, headers: ::header::HeaderMap) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            for (key, value) in headers.iter() {
                req.headers_mut().insert(key, value.clone());
            }
        }
        self
    }

    /// Enable HTTP basic authentication.
    pub fn basic_auth<U, P>(&mut self, username: U, password: Option<P>) -> &mut RequestBuilder
    where
        U: Into<String>,
        P: Into<String>,
    {
        let username = username.into();
        let password = password.map(|p| p.into()).unwrap_or(String::new());
        let header_value = format!("basic {}:{}", username, encode(&password));
        self.header(::header::AUTHORIZATION, HeaderValue::from_str(header_value.as_str()).expect(""))
    }

    /// Set the request body.
    pub fn body<T: Into<Body>>(&mut self, body: T) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            *req.body_mut() = Some(body.into());
        }
        self
    }

    /// Modify the query string of the URL.
    ///
    /// Modifies the URL of this request, adding the parameters provided.
    /// This method appends and does not overwrite. This means that it can
    /// be called multiple times and that existing query parameters are not
    /// overwritten if the same key is used. The key will simply show up
    /// twice in the query string.
    /// Calling `.query([("foo", "a"), ("foo", "b")])` gives `"foo=a&boo=b"`.
    ///
    /// # Note
    /// This method does not support serializing a single key-value
    /// pair. Instead of using `.query(("key", "val"))`, use a sequence, such
    /// as `.query(&[("key", "val")])`. It's also possible to serialize structs
    /// and maps into a key-value pair.
    ///
    /// # Errors
    /// This method will fail if the object you provide cannot be serialized
    /// into a query string.
    pub fn query<T: Serialize + ?Sized>(&mut self, query: &T) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            let url = req.url_mut();
            let mut pairs = url.query_pairs_mut();
            let serializer = serde_urlencoded::Serializer::new(&mut pairs);

            if let Err(err) = query.serialize(serializer) {
                self.err = Some(::error::from(err));
            }
        }
        self
    }

    /// Send a form body.
    pub fn form<T: Serialize + ?Sized>(&mut self, form: &T) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            match serde_urlencoded::to_string(form) {
                Ok(body) => {
                    req.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_str(mime::APPLICATION_WWW_FORM_URLENCODED.as_ref()).expect(""));
                    *req.body_mut() = Some(body.into());
                },
                Err(err) => self.err = Some(::error::from(err)),
            }
        }
        self
    }

    /// Send a JSON body.
    ///
    /// # Errors
    ///
    /// Serialization can fail if `T`'s implementation of `Serialize` decides to
    /// fail, or if `T` contains a map with non-string keys.
    pub fn json<T: Serialize + ?Sized>(&mut self, json: &T) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            match serde_json::to_vec(json) {
                Ok(body) => {
                    req.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_str(mime::APPLICATION_JSON.as_ref()).expect(""));
                    *req.body_mut() = Some(body.into());
                },
                Err(err) => self.err = Some(::error::from(err)),
            }
        }
        self
    }

    /// Build a `Request`, which can be inspected, modified and executed with
    /// `Client::execute()`.
    ///
    /// # Panics
    ///
    /// This method consumes builder internal state. It panics on an attempt to
    /// reuse already consumed builder.
    pub fn build(&mut self) -> ::Result<Request> {
        if let Some(err) = self.err.take() {
            Err(err)
        } else {
            Ok(self.request
                .take()
                .expect("RequestBuilder cannot be reused after builder a Request"))
        }
    }

    /// Constructs the Request and sends it the target URL, returning a Response.
    ///
    /// # Errors
    ///
    /// This method fails if there was an error while sending request,
    /// redirect loop was detected or redirect limit was exhausted.
    pub fn send(&mut self) -> Pending {
        match self.build() {
            Ok(req) => self.client.execute(req),
            Err(err) => pending_err(err),
        }
    }
}

fn request_mut<'a>(req: &'a mut Option<Request>, err: &Option<::Error>) -> Option<&'a mut Request> {
    if err.is_some() {
        None
    } else {
        req.as_mut()
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
pub fn builder(client: Client, req: ::Result<Request>) -> RequestBuilder {
    match req {
        Ok(req) => RequestBuilder {
            client: client,
            request: Some(req),
            err: None,
        },
        Err(err) => RequestBuilder {
            client: client,
            request: None,
            err: Some(err)
        },
    }
}

#[inline]
pub fn pieces(req: Request) -> (Method, Url, HeaderMap, Option<Body>) {
    (req.method, req.url, req.headers, req.body)
}

#[cfg(test)]
mod tests {
    use super::Client;
    use std::collections::BTreeMap;
    use tokio_core::reactor::Core;

    #[test]
    fn add_query_append() {
        let mut core = Core::new().unwrap();
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.get(some_url);

        r.query(&[("foo", "bar")]);
        r.query(&[("qux", 3)]);

        let req = r.build().expect("request is valid");
        assert_eq!(req.url().query(), Some("foo=bar&qux=3"));
    }

    #[test]
    fn add_query_append_same() {
        let mut core = Core::new().unwrap();
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.get(some_url);

        r.query(&[("foo", "a"), ("foo", "b")]);

        let req = r.build().expect("request is valid");
        assert_eq!(req.url().query(), Some("foo=a&foo=b"));
    }

    #[test]
    fn add_query_struct() {
        #[derive(Serialize)]
        struct Params {
            foo: String,
            qux: i32,
        }

        let mut core = Core::new().unwrap();
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.get(some_url);

        let params = Params { foo: "bar".into(), qux: 3 };

        r.query(&params);

        let req = r.build().expect("request is valid");
        assert_eq!(req.url().query(), Some("foo=bar&qux=3"));
    }

    #[test]
    fn add_query_map() {
        let mut params = BTreeMap::new();
        params.insert("foo", "bar");
        params.insert("qux", "three");

        let mut core = Core::new().unwrap();
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.get(some_url);

        r.query(&params);

        let req = r.build().expect("request is valid");
        assert_eq!(req.url().query(), Some("foo=bar&qux=three"));
    }

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
