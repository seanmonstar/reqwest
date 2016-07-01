extern crate hyper;

#[macro_use] extern crate log;

pub use hyper::{Method, StatusCode, header, Url};
pub use self::client::{Client, Response};

mod client;
mod sync;

pub fn get(url: &str) -> Result<Response, String> {
    let client = Client::new();
    client.get(url).send()
}
