#![allow(warnings)]


extern crate hyper;

#[macro_use] extern crate log;

pub use hyper::{Method, StatusCode, header, Url};

pub use self::client::{Client, Response};
pub use self::error::{Error, Result};

mod body;
mod client;
mod error;
mod sync;

pub fn get(url: &str) -> ::Result<Response> {
    let client = Client::new();
    client.get(url).send()
}
