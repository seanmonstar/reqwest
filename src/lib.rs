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
//! - Plain bodies, JSON, urlencoded, multipart
//! - Customizable redirect policy
//! - Cookies
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
extern crate hyper;

#[macro_use] extern crate log;
#[cfg(feature = "tls")] extern crate native_tls;
extern crate serde;
extern crate serde_json;
extern crate url;

pub use hyper::header;
pub use hyper::method::Method;
pub use hyper::status::StatusCode;
pub use hyper::version::HttpVersion;
pub use hyper::Url;
pub use url::ParseError as UrlError;

pub use self::client::{Client, Response};
pub use self::error::{Error, Result};

mod body;
mod client;
mod error;

#[cfg(feature = "tls")] mod tls;

/// Shortcut method to quickly make a `GET` request.
pub fn get(url: &str) -> ::Result<Response> {
    let client = try!(Client::new());
    client.get(url).send()
}
