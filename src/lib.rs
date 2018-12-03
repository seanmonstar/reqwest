#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![cfg_attr(test, deny(warnings))]
#![cfg_attr(docs_rs_workaround, feature(extern_prelude))]
#![doc(html_root_url = "https://docs.rs/reqwest/0.9.5")]

//! # reqwest
//!
//! The `reqwest` crate provides a convenient, higher-level HTTP Client.
//!
//! It handles many of the things that most people just expect an HTTP client
//! to do for them.
//!
//! - Uses system-native TLS
//! - Plain bodies, JSON, urlencoded, multipart
//! - Customizable redirect policy
//! - Proxies
//! - Cookies (only rudimentary support, full support is TODO)
//!
//! The rudimentary cookie support means that the cookies need to be manually
//! configured for every single request. In other words, there's no cookie jar
//! support as of now. The tracking issue for this feature is available
//! [on GitHub][cookiejar_issue].
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
//! # use reqwest::{Error, Response};
//!
//! # fn run() -> Result<(), Error> {
//! let body = reqwest::get("https://www.rust-lang.org")?
//!     .text()?;
//!
//! println!("body = {:?}", body);
//! # Ok(())
//! # }
//! ```
//!
//! Additionally, reqwest's [`Response`][response] struct implements Rust's
//! `Read` trait, so many useful standard library and third party crates will
//! have convenience methods that take a `Response` anywhere `T: Read` is
//! acceptable.
//!
//! **NOTE**: If you plan to perform multiple requests, it is best to create a
//! [`Client`][client] and reuse it, taking advantage of keep-alive connection
//! pooling.
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
//! let client = reqwest::Client::new();
//! let res = client.post("http://httpbin.org/post")
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
//! let client = reqwest::Client::new();
//! let res = client.post("http://httpbin.org/post")
//!     .form(&params)
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
//! let client = reqwest::Client::new();
//! let res = client.post("http://httpbin.org/post")
//!     .json(&map)
//!     .send()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Optional Features
//!
//! The following are a list of [Cargo features][cargo-features] that can be
//! enabled or disabled:
//!
//! - **default-tls** *(enabled by default)*: Provides TLS support via the
//!   `native-tls` library to connect over HTTPS.
//! - **hyper-011**: Provides support for hyper's old typed headers.
//!
//!
//! [hyper]: http://hyper.rs
//! [client]: ./struct.Client.html
//! [response]: ./struct.Response.html
//! [get]: ./fn.get.html
//! [builder]: ./struct.RequestBuilder.html
//! [serde]: http://serde.rs
//! [cookiejar_issue]: https://github.com/seanmonstar/reqwest/issues/14
//! [cargo-features]: https://doc.rust-lang.org/stable/cargo/reference/manifest.html#the-features-section

extern crate base64;
extern crate bytes;
extern crate encoding_rs;
#[macro_use]
extern crate futures;
extern crate http;
extern crate hyper;
#[cfg(feature = "hyper-011")]
pub extern crate hyper_old_types as hyper_011;
#[cfg(feature = "default-tls")]
extern crate hyper_tls;
#[macro_use]
extern crate log;
extern crate libflate;
extern crate mime;
extern crate mime_guess;
#[cfg(feature = "default-tls")]
extern crate native_tls;
extern crate serde;
#[cfg(test)]
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_urlencoded;
extern crate tokio;
#[cfg_attr(feature = "default-tls", macro_use)]
extern crate tokio_io;
extern crate url;
extern crate uuid;

#[cfg(feature = "rustls-tls")]
extern crate hyper_rustls;
#[cfg(feature = "rustls-tls")]
extern crate tokio_rustls;
#[cfg(feature = "rustls-tls")]
extern crate webpki_roots;
#[cfg(feature = "rustls-tls")]
extern crate rustls;

pub use hyper::header;
pub use hyper::Method;
pub use hyper::{StatusCode, Version};
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
#[cfg(feature = "tls")]
pub use self::tls::{Certificate, Identity};


// this module must be first because of the `try_` macro
#[macro_use]
mod error;

mod async_impl;
mod connect;
#[cfg(feature = "default-tls")]
mod connect_async;
mod body;
mod client;
mod into_url;
mod proxy;
mod redirect;
mod request;
mod response;
#[cfg(feature = "tls")]
mod tls;
mod wait;

pub mod multipart;

/// An 'async' implementation of the reqwest `Client`.
///
/// Relies on the `futures` crate, which is unstable, hence this module
/// is **unstable**.
pub mod async {
    pub use ::async_impl::{
        Body,
        Chunk,
        Decoder,
        Client,
        ClientBuilder,
        Request,
        RequestBuilder,
        Response,
        ResponseBuilderExt,
    };
}

/// Shortcut method to quickly make a `GET` request.
///
/// See also the methods on the [`reqwest::Response`](./struct.Response.html)
/// type.
///
/// **NOTE**: This function creates a new internal `Client` on each call,
/// and so should not be used if making many requests. Create a
/// [`Client`](./struct.Client.html) instead.
///
/// # Examples
///
/// ```rust
/// # fn run() -> Result<(), reqwest::Error> {
/// let body = reqwest::get("https://www.rust-lang.org")?
///     .text()?;
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
    Client::new()
        .get(url)
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
