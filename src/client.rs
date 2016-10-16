use std::io::{self, Read};

use hyper::header::Headers;
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::version::HttpVersion;
use hyper::{Url};

use ::body::Body;

pub struct Client {
    inner: ::hyper::Client,
}

impl Client {
    pub fn new() -> Client {
        Client {
            inner: ::hyper::Client::new(),
        }
    }

    pub fn get(&self, url: &str) -> RequestBuilder {
        self.request(Method::Get, Url::parse(url).unwrap())
    }

    pub fn request(&self, method: Method, url: Url) -> RequestBuilder {
        debug!("request {:?} \"{}\"", method, url);
        RequestBuilder {
            client: self,
            method: method,
            url: url,
            version: HttpVersion::Http11,
            headers: Headers::new(),

            body: None,
        }
    }
}

pub struct RequestBuilder<'a> {
    client: &'a Client,

    method: Method,
    url: Url,
    version: HttpVersion,
    headers: Headers,

    body: Option<Body>,
}

impl<'a> RequestBuilder<'a> {

    pub fn header<H: ::header::Header + ::header::HeaderFormat>(mut self, header: H) -> RequestBuilder<'a> {
        self.headers.set(header);
        self
    }

    pub fn headers(mut self, headers: ::header::Headers) -> RequestBuilder<'a> {
        self.headers.extend(headers.iter());
        self
    }

    pub fn body<T: Into<Body>>(mut self, body: T) -> RequestBuilder<'a> {
        self.body = Some(body.into());
        self
    }

    pub fn send(mut self) -> ::Result<Response> {
        let req = self.client.inner.request(self.method, self.url)
            .headers(self.headers);

        //TODO: body

        let res = try!(req.send());
        Ok(Response {
            inner: res
        })
    }
}

pub struct Response {
    inner: ::hyper::client::Response,
}

impl Response {
    pub fn status(&self) -> &StatusCode {
        &self.inner.status
    }

    pub fn headers(&self) -> &Headers {
        &self.inner.headers
    }

    pub fn version(&self) -> &HttpVersion {
        &self.inner.version
    }
}

impl Read for Response {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}
