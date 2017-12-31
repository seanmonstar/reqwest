use std::fmt;

use hyper::header::ContentType;
use serde::Serialize;
use serde_json;
use serde_urlencoded;

use body::{self, Body};
use header::Headers;
use {async_impl, Client, Method, Url};

/// A request which can be executed with `Client::execute()`.
pub struct Request {
    body: Option<Body>,
    inner: async_impl::Request,
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
            body: None,
            inner: async_impl::Request::new(method, url),
        }
    }

    /// Get the method.
    #[inline]
    pub fn method(&self) -> &Method {
        self.inner.method()
    }

    /// Get a mutable reference to the method.
    #[inline]
    pub fn method_mut(&mut self) -> &mut Method {
        self.inner.method_mut()
    }

    /// Get the url.
    #[inline]
    pub fn url(&self) -> &Url {
        self.inner.url()
    }

    /// Get a mutable reference to the url.
    #[inline]
    pub fn url_mut(&mut self) -> &mut Url {
        self.inner.url_mut()
    }

    /// Get the headers.
    #[inline]
    pub fn headers(&self) -> &Headers {
        self.inner.headers()
    }

    /// Get a mutable reference to the headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut Headers {
        self.inner.headers_mut()
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
    ///
    /// ```rust
    /// use reqwest::header::UserAgent;
    ///
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = reqwest::Client::new();
    /// let res = client.get("https://www.rust-lang.org")
    ///     .header(UserAgent::new("foo"))
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn header<H>(&mut self, header: H) -> &mut RequestBuilder
    where
        H: ::header::Header,
    {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            req.headers_mut().set(header);
        }
        self
    }

    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    ///
    /// ```rust
    /// use reqwest::header::{Headers, UserAgent, ContentType};
    /// # use std::fs;
    ///
    /// fn construct_headers() -> Headers {
    ///     let mut headers = Headers::new();
    ///     headers.set(UserAgent::new("reqwest"));
    ///     headers.set(ContentType::png());
    ///     headers
    /// }
    ///
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let file = fs::File::open("much_beauty.png")?;
    /// let client = reqwest::Client::new();
    /// let res = client.post("http://httpbin.org/post")
    ///     .headers(construct_headers())
    ///     .body(file)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn headers(&mut self, headers: ::header::Headers) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            req.headers_mut().extend(headers.iter());
        }
        self
    }

    /// Enable HTTP basic authentication.
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = reqwest::Client::new();
    /// let resp = client.delete("http://httpbin.org/delete")
    ///     .basic_auth("admin", Some("good password"))
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
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
    ///
    /// # Examples
    ///
    /// Using a string:
    ///
    /// ```rust
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let client = reqwest::Client::new();
    /// let res = client.post("http://httpbin.org/post")
    ///     .body("from a &str!")
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Using a `File`:
    ///
    /// ```rust
    /// # use std::fs;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// let file = fs::File::open("from_a_file.txt")?;
    /// let client = reqwest::Client::new();
    /// let res = client.post("http://httpbin.org/post")
    ///     .body(file)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Using arbitrary bytes:
    ///
    /// ```rust
    /// # use std::fs;
    /// # fn run() -> Result<(), Box<::std::error::Error>> {
    /// // from bytes!
    /// let bytes: Vec<u8> = vec![1, 10, 100];
    /// let client = reqwest::Client::new();
    /// let res = client.post("http://httpbin.org/post")
    ///     .body(bytes)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
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
    /// ```rust
    /// # use reqwest::Error;
    /// #
    /// # fn run() -> Result<(), Error> {
    /// let client = reqwest::Client::new();
    /// let res = client.get("http://httpbin.org")
    ///     .query(&[("lang", "rust")])
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
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
    pub fn query<T: Serialize>(&mut self, query: &T) -> &mut RequestBuilder {
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
    /// let client = reqwest::Client::new();
    /// let res = client.post("http://httpbin.org")
    ///     .form(&params)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// This method fails if the passed value cannot be serialized into
    /// url encoded format
    pub fn form<T: Serialize>(&mut self, form: &T) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            match serde_urlencoded::to_string(form) {
                Ok(body) => {
                    req.headers_mut().set(ContentType::form_url_encoded());
                    *req.body_mut() = Some(body.into());
                },
                Err(err) => self.err = Some(::error::from(err)),
            }
        }
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
    /// let client = reqwest::Client::new();
    /// let res = client.post("http://httpbin.org")
    ///     .json(&map)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Serialization can fail if `T`'s implementation of `Serialize` decides to
    /// fail, or if `T` contains a map with non-string keys.
    pub fn json<T: Serialize>(&mut self, json: &T) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            match serde_json::to_vec(json) {
                Ok(body) => {
                    req.headers_mut().set(ContentType::json());
                    *req.body_mut() = Some(body.into());
                },
                Err(err) => self.err = Some(::error::from(err)),
            }
        }
        self
    }

    /// Sends a multipart/form-data body.
    ///
    /// ```
    /// use reqwest::mime;
    /// # use reqwest::Error;
    ///
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let client = reqwest::Client::new();
    /// let form = reqwest::multipart::Form::new()
    ///     .text("key3", "value3")
    ///     .file("file", "/path/to/field")?;
    ///
    /// let response = client.post("your url")
    ///     .multipart(form)
    ///     .send()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// See [`multipart`](multipart/) for more examples.
    pub fn multipart(&mut self, mut multipart: ::multipart::Form) -> &mut RequestBuilder {
        if let Some(req) = request_mut(&mut self.request, &self.err) {
            req.headers_mut().set(
                ::header::ContentType(format!("multipart/form-data; boundary={}", ::multipart_::boundary(&multipart))
                    .parse().unwrap()
                )
            );
            *req.body_mut() = Some(match ::multipart_::compute_length(&mut multipart) {
                Some(length) => Body::sized(::multipart_::reader(multipart), length),
                None => Body::new(::multipart_::reader(multipart)),
            })
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
    pub fn send(&mut self) -> ::Result<::Response> {
        let request = self.build()?;
        self.client.execute(request)
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
    f.field("method", req.method())
        .field("url", req.url())
        .field("headers", req.headers())
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
pub fn async(req: Request) -> (async_impl::Request, Option<body::Sender>) {
    use header::ContentLength;

    let mut req_async = req.inner;
    let body = req.body.and_then(|body| {
        let (tx, body, len) = body::async(body);
        if let Some(len) = len {
            req_async.headers_mut().set(ContentLength(len));
        }
        *req_async.body_mut() = Some(body);
        tx
    });
    (req_async, body)
}

#[cfg(test)]
mod tests {
    use {body, Client, Method};
    use header::{Host, Headers, ContentType};
    use std::collections::{BTreeMap, HashMap};
    use serde_json;
    use serde_urlencoded;

    #[test]
    fn basic_get_request() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let r = client.get(some_url).build().unwrap();

        assert_eq!(r.method(), &Method::Get);
        assert_eq!(r.url().as_str(), some_url);
    }

    #[test]
    fn basic_head_request() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let r = client.head(some_url).build().unwrap();

        assert_eq!(r.method(), &Method::Head);
        assert_eq!(r.url().as_str(), some_url);
    }

    #[test]
    fn basic_post_request() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let r = client.post(some_url).build().unwrap();

        assert_eq!(r.method(), &Method::Post);
        assert_eq!(r.url().as_str(), some_url);
    }

    #[test]
    fn basic_put_request() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let r = client.put(some_url).build().unwrap();

        assert_eq!(r.method(), &Method::Put);
        assert_eq!(r.url().as_str(), some_url);
    }

    #[test]
    fn basic_patch_request() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let r = client.patch(some_url).build().unwrap();

        assert_eq!(r.method(), &Method::Patch);
        assert_eq!(r.url().as_str(), some_url);
    }

    #[test]
    fn basic_delete_request() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let r = client.delete(some_url).build().unwrap();

        assert_eq!(r.method(), &Method::Delete);
        assert_eq!(r.url().as_str(), some_url);
    }

    #[test]
    fn add_header() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let header = Host::new("google.com", None);

        // Add a copy of the header to the request builder
        let r = r.header(header.clone()).build().unwrap();

        // then check it was actually added
        assert_eq!(r.headers().get::<Host>(), Some(&header));
    }

    #[test]
    fn add_headers() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let header = Host::new("google.com", None);

        let mut headers = Headers::new();
        headers.set(header);

        // Add a copy of the headers to the request builder
        let r = r.headers(headers.clone()).build().unwrap();

        // then make sure they were added correctly
        assert_eq!(r.headers(), &headers);
    }

    #[test]
    fn add_body() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let body = "Some interesting content";

        let mut r = r.body(body).build().unwrap();

        let buf = body::read_to_string(r.body_mut().take().unwrap()).unwrap();

        assert_eq!(buf, body);
    }

    #[test]
    fn add_query_append() {
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

        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.get(some_url);

        r.query(&params);

        let req = r.build().expect("request is valid");
        assert_eq!(req.url().query(), Some("foo=bar&qux=three"));
    }

    #[test]
    fn add_form() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let mut form_data = HashMap::new();
        form_data.insert("foo", "bar");

        let mut r = r.form(&form_data).build().unwrap();

        // Make sure the content type was set
        assert_eq!(r.headers().get::<ContentType>(),
                   Some(&ContentType::form_url_encoded()));

        let buf = body::read_to_string(r.body_mut().take().unwrap()).unwrap();

        let body_should_be = serde_urlencoded::to_string(&form_data).unwrap();
        assert_eq!(buf, body_should_be);
    }

    #[test]
    fn add_json() {
        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);

        let mut json_data = HashMap::new();
        json_data.insert("foo", "bar");

        let mut r = r.json(&json_data).build().unwrap();

        // Make sure the content type was set
        assert_eq!(r.headers().get::<ContentType>(), Some(&ContentType::json()));

        let buf = body::read_to_string(r.body_mut().take().unwrap()).unwrap();

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

        let client = Client::new();
        let some_url = "https://google.com/";
        let mut r = client.post(some_url);
        let json_data = MyStruct;
        assert!(r.json(&json_data).build().unwrap_err().is_serialization());
    }
}
