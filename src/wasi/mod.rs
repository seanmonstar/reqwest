mod body;
mod client;
mod request;
mod response;

#[cfg(feature = "multipart")]
pub mod multipart;

pub use body::Body;
pub use client::{Client, ClientBuilder};
pub use request::{Request, RequestBuilder};
pub use response::Response;

/// Shortcut method to quickly make a `GET` request.
///
/// **NOTE**: This function creates a new internal `Client` on each call,
/// and so should not be used if making many requests. Create a
/// [`Client`](./struct.Client.html) instead.
///
/// # Examples
///
/// ```rust
/// # fn run() -> Result<(), reqwest::Error> {
/// let body = reqwest::blocking::get("https://www.rust-lang.org")?
///     .text()?;
/// # Ok(())
/// # }
/// # fn main() { }
/// ```
///
pub fn get<T: crate::IntoUrl>(url: T) -> crate::Result<Response> {
    Client::builder().build()?.get(url).send()
}
