#![deny(warnings)]
#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![doc(html_root_url = "https://docs.rs/reqwest/0.7.3")]

//! # reqwest
//!
//! The `reqwest` crate provides a convenient, higher-level HTTP Client.
//!
//! It handles many of the things that most people just expect an HTTP client
//! to do for them.
//!
//! - Uses system-native TLS
//! - Plain bodies, JSON, urlencoded, (TODO: multipart)
//! - Customizable redirect policy
//! - Proxies
//! - (TODO: Cookies)
//!
//! The `reqwest::Client` is synchronous, making it a great fit for
//! applications that only require a few HTTP requests, and wish to handle
//! them synchronously.
//!
//! ## Making a GET request
//!
//! For a single request, you can use the [`get`][get] shortcut method.
//!
//! ```rust
//! use std::io::Read;
//! # use reqwest::{Error, Response};
//!
//! # fn run() -> Result<Response, Error> {
//! let mut resp = reqwest::get("https://www.rust-lang.org")?;
//! assert!(resp.status().is_success());
//!
//! let mut content = String::new();
//! resp.read_to_string(&mut content);
//! # Ok(resp)
//! # }
//! ```
//!
//! As you can see, reqwest's [`Response`][response] struct implements Rust's
//! `Read` trait, so many useful standard library and third party crates will
//! have convenience methods that take a `Response` anywhere `T: Read` is
//! acceptable.
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
//! ```rust
//! # use reqwest::Error;
//! #
//! # fn run() -> Result<(), Error> {
//! let client = reqwest::Client::new()?;
//! let res = client.post("http://httpbin.org/post")?
//!     .body("the exact body that is sent")
//!     .send()?;
//! # Ok(())
//! # }
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
//! ```rust
//! # use reqwest::Error;
//! #
//! # fn run() -> Result<(), Error> {
//! // This will POST a body of `foo=bar&baz=quux`
//! let params = [("foo", "bar"), ("baz", "quux")];
//! let client = reqwest::Client::new()?;
//! let res = client.post("http://httpbin.org/post")?
//!     .form(&params)?
//!     .send()?;
//! # Ok(())
//! # }
//! ```
//!
//! ### JSON
//!
//! There is also a `json` method helper on the [`RequestBuilder`][builder] that works in
//! a similar fashion the `form` method. It can take any value that can be
//! serialized into JSON.
//!
//! ```rust
//! # use reqwest::Error;
//! # use std::collections::HashMap;
//! #
//! # fn run() -> Result<(), Error> {
//! // This will POST a body of `{"lang":"rust","body":"json"}`
//! let mut map = HashMap::new();
//! map.insert("lang", "rust");
//! map.insert("body", "json");
//!
//! let client = reqwest::Client::new()?;
//! let res = client.post("http://httpbin.org/post")?
//!     .json(&map)?
//!     .send()?;
//! # Ok(())
//! # }
//! ```
//!
//! [hyper]: http://hyper.rs
//! [client]: ./struct.Client.html
//! [response]: ./struct.Response.html
//! [get]: ./fn.get.html
//! [builder]: ./client/struct.RequestBuilder.html
//! [serde]: http://serde.rs

extern crate bytes;
#[macro_use]
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
#[macro_use]
extern crate log;
extern crate libflate;
extern crate native_tls;
extern crate serde;
extern crate serde_json;
extern crate serde_urlencoded;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_tls;
extern crate url;

pub use hyper::header;
pub use hyper::mime;
pub use hyper::Method;
pub use hyper::StatusCode;
pub use url::Url;
pub use url::ParseError as UrlError;

pub use self::client::{Client, ClientBuilder};
pub use self::error::{Error, Result};
pub use self::body::Body;
pub use self::into_url::IntoUrl;
pub use self::proxy::Proxy;
pub use self::redirect::{RedirectAction, RedirectAttempt, RedirectPolicy};
pub use self::request::{Request, RequestBuilder};
pub use self::response::Response;
pub use self::tls::Certificate;


// this module must be first because of the `try_` macro
#[macro_use]
mod error;

/// A set of unstable functionality.
///
/// This module is only available when the `unstable` feature is enabled.
/// There is no backwards compatibility guarantee for any of the types within.
#[cfg(feature = "unstable")]
pub mod unstable {
    /// An 'async' implementation of the reqwest `Client`.
    ///
    /// Relies on the `futures` crate, which is unstable, hence this module
    /// is unstable.
    pub mod async {
        pub use ::async_impl::{
            Body,
            Chunk,
            Client,
            ClientBuilder,
            Request,
            RequestBuilder,
            Response,
        };
    }
}


mod async_impl;
mod connect;
mod body;
mod client;
mod into_url;
mod proxy;
mod redirect;
mod request;
mod response;
mod tls;
mod wait;


/// Shortcut method to quickly make a `GET` request.
///
/// See also the methods on the [`reqwest::Response`](./struct.Response.html)
/// type.
///
/// # Examples
///
/// ```rust
/// use std::io::Read;
///
/// # fn run() -> Result<(), Box<::std::error::Error>> {
/// let mut result = String::new();
/// reqwest::get("https://www.rust-lang.org")?
///     .read_to_string(&mut result)?;
/// # Ok(())
/// # }
/// # fn main() { }
/// ```
///
/// # Errors
///
/// This function fails if:
///
/// - native TLS backend cannot be initialized
/// - supplied `Url` cannot be parsed
/// - there was an error while sending request
/// - redirect loop was detected
/// - redirect limit was exhausted
pub fn get<T: IntoUrl>(url: T) -> ::Result<Response> {
    Client::new()?
        .get(url)?
        .send()
}

fn _assert_impls() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn assert_clone<T: Clone>() {}

    assert_send::<Client>();
    assert_sync::<Client>();
    assert_clone::<Client>();

    assert_send::<Request>();
    assert_send::<RequestBuilder>();

    assert_send::<Response>();

    assert_send::<Error>();
    assert_sync::<Error>();
}
