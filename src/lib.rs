#![deny(warnings)]
#![deny(missing_docs)]

//! # reqwest
//!
//! The `reqwest` crate provides a convenient, higher-level HTTP Client.
//!
//! It handles many of the things that most people just expect an HTTP client
//! to do for them.
//!
//! - Uses system-native TLS
//! - Plain bodies, JSON, urlencoded, (TODO: multipart)
//! - (TODO: Customizable redirect policy)
//! - (TODO: Cookies)
//!
//! The `reqwest::Client` is synchronous, making it a great fit for
//! applications that only require a few HTTP requests, and wish to handle
//! them synchronously. When [hyper][] releases with asynchronous support,
//! `reqwest` will be updated to use it internally, but still provide a
//! synchronous Client, for convenience. A `reqwest::async::Client` will also
//! be added.
//!
//! ## Making a GET request
//!
//! For a single request, you can use the `get` shortcut method.
//!
//!
//! ```no_run
//! let resp = reqwest::get("https://www.rust-lang.org").unwrap();
//! assert!(resp.status().is_success());
//! ```
//!
//! If you plan to perform multiple requests, it is best to create a [`Client`][client]
//! and reuse it, taking advantage of keep-alive connection pooling.
//!
//! ## Making POST requests (or setting request bodies)
//!
//! There are several ways you can set the body of a request. The basic one is
//! by using the `body()` method of a [`RequestBuilder`][builder]. This lets you set the
//! exact raw bytes of what the body should be. It accepts various types,
//! including `String`, `Vec<u8>`, and `File`. If you wish to pass a custom
//! Reader, you can use the `reqwest::Body::new()` constructor.
//!
//! ```no_run
//! let client = reqwest::Client::new().unwrap();
//! let res = client.post("http://httpbin.org/post")
//!     .body("the exact body that is sent")
//!     .send();
//! ```
//!
//! ### Forms
//!
//! It's very common to want to send form data in a request body. This can be
//! done with any type that can be serialized into form data.
//!
//! This can be an array of tuples, or a `HashMap`, or a custom type that
//! implements [`Serialize`][serde].
//!
//! ```no_run
//! // This will POST a body of `foo=bar&baz=quux`
//! let params = [("foo", "bar"), ("baz", "quux")];
//! let client = reqwest::Client::new().unwrap();
//! let res = client.post("http://httpbin.org/post")
//!     .form(&params)
//!     .send();
//! ```
//!
//! ### JSON
//!
//! There is also a `json` method helper on the [`RequestBuilder`][builder] that works in
//! a similar fashion the `form` method. It can take any value that can be
//! serialized into JSON.
//!
//! ```no_run
//! # use std::collections::HashMap;
//! // This will POST a body of `{"lang":"rust","body":"json"}`
//! let mut map = HashMap::new();
//! map.insert("lang", "rust");
//! map.insert("body", "json");
//!
//! let client = reqwest::Client::new().unwrap();
//! let res = client.post("http://httpbin.org/post")
//!     .json(&map)
//!     .send();
//! ```
//!
//! [hyper]: http://hyper.rs
//! [client]: ./struct.Client.html
//! [builder]: ./client/struct.RequestBuilder.html
//! [serde]: http://serde.rs
extern crate hyper;

#[macro_use] extern crate log;
extern crate native_tls;
extern crate serde;
extern crate serde_json;
extern crate serde_urlencoded;
extern crate url;

pub use hyper::client::IntoUrl;
pub use hyper::header;
pub use hyper::method::Method;
pub use hyper::status::StatusCode;
pub use hyper::version::HttpVersion;
pub use hyper::Url;
pub use url::ParseError as UrlError;

pub use self::client::{Client, Response, RequestBuilder};
pub use self::error::{Error, Result};
pub use self::body::Body;

mod body;
mod client;
mod error;
mod tls;


/// Shortcut method to quickly make a `GET` request.
pub fn get<T: IntoUrl>(url: T) -> ::Result<Response> {
    let client = try!(Client::new());
    client.get(url).send()
}

fn _assert_impls() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<Client>();
    assert_sync::<Client>();

    assert_send::<RequestBuilder>();
    assert_send::<Response>();
}
