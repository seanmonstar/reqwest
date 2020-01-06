use http::Method;
use js_sys::Uint8Array;
use std::future::Future;
use wasm_bindgen::UnwrapThrowExt as _;

use super::{Request, RequestBuilder, Response};
use crate::IntoUrl;

/// dox
#[derive(Clone, Debug)]
pub struct Client(());

/// dox
#[derive(Debug)]
pub struct ClientBuilder(());

impl Client {
    /// dox
    pub fn new() -> Self {
        Client::builder().build().unwrap_throw()
    }

    /// dox
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Convenience method to make a `GET` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    /// Convenience method to make a `POST` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    /// Convenience method to make a `PUT` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn put<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PUT, url)
    }

    /// Convenience method to make a `PATCH` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn patch<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PATCH, url)
    }

    /// Convenience method to make a `DELETE` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn delete<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }

    /// Convenience method to make a `HEAD` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn head<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }

    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// request body before sending.
    ///
    /// # Errors
    ///
    /// This method fails whenever supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        let req = url.into_url().map(move |url| Request::new(method, url));
        RequestBuilder::new(self.clone(), req)
    }

    pub(super) fn execute_request(
        &self,
        req: Request,
    ) -> impl Future<Output = crate::Result<Response>> {
        fetch(req)
    }
}

async fn fetch(req: Request) -> crate::Result<Response> {
    // Build the js Request
    let mut init = web_sys::RequestInit::new();
    init.method(req.method().as_str());

    let js_headers = web_sys::Headers::new()
        .map_err(crate::error::wasm)
        .map_err(crate::error::builder)?;

    for (name, value) in req.headers() {
        js_headers
            .append(
                name.as_str(),
                value.to_str().map_err(crate::error::builder)?,
            )
            .map_err(crate::error::wasm)
            .map_err(crate::error::builder)?;
    }
    init.headers(&js_headers.into());

    // When req.cors is true, do nothing because the default mode is 'cors'
    if !req.cors {
        init.mode(web_sys::RequestMode::NoCors);
    }

    if let Some(body) = req.body() {
        let body_bytes: &[u8] = body.bytes();
        let body_array: Uint8Array = body_bytes.into();
        init.body(Some(&body_array.into()));
    }

    let js_req = web_sys::Request::new_with_str_and_init(req.url().as_str(), &init)
        .map_err(crate::error::wasm)
        .map_err(crate::error::builder)?;

    // Await the fetch() promise
    let p = web_sys::window()
        .expect("window should exist")
        .fetch_with_request(&js_req);
    let js_resp = super::promise::<web_sys::Response>(p)
        .await
        .map_err(crate::error::request)?;

    // Convert from the js Response
    let mut resp = http::Response::builder()
        .status(js_resp.status());

    let js_headers = js_resp.headers();
    let js_iter = js_sys::try_iter(&js_headers)
        .expect_throw("headers try_iter")
        .expect_throw("headers have an iterator");

    for item in js_iter {
        let item = item.expect_throw("headers iterator doesn't throw");
        let v: Vec<String> = item.into_serde().expect_throw("headers into_serde");
        resp = resp.header(
            v.get(0).expect_throw("headers name"),
            v.get(1).expect_throw("headers value"),
        );
    }

    resp.body(js_resp)
        .map(Response::new)
        .map_err(crate::error::request)
}

// ===== impl ClientBuilder =====

impl ClientBuilder {
    /// dox
    pub fn new() -> Self {
        ClientBuilder(())
    }

    /// dox
    pub fn build(self) -> Result<Client, crate::Error> {
        Ok(Client(()))
    }
}
