#![allow(warnings)]


extern crate hyper;

#[macro_use] extern crate log;

pub use hyper::header;
pub use hyper::method::Method;
pub use hyper::status::StatusCode;
pub use hyper::version::HttpVersion;
pub use hyper::Url;

pub use self::client::{Client, Response};
pub use self::error::{Error, Result};

mod body;
mod client;
mod error;

pub fn get(url: &str) -> ::Result<Response> {
    let client = Client::new();
    client.get(url).send()
}
