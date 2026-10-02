//! Types for creating custom connectors.
//!
//! This module provides the types needed to implement a custom
//! connection service for use with [`crate::ClientBuilder::custom_connector`].
//!
//! # Example
//!
//! ```rust,ignore
//! use reqwest::connector::{BoxError, Connection, Connected};
//! use hyper::rt::{Read, Write};
//! use http::Uri;
//! use std::future::Future;
//! use std::pin::Pin;
//! use std::task::{Context, Poll};
//! use tower_service::Service;
//!
//! #[derive(Clone)]
//! struct MyConnector { /* ... */ }
//!
//! // Your connection type must implement Read, Write, and Connection
//! struct MyConnection { /* ... */ }
//!
//! impl Connection for MyConnection {
//!     fn connected(&self) -> Connected {
//!         Connected::new()
//!     }
//! }
//!
//! // Implement hyper::rt::Read and hyper::rt::Write for MyConnection...
//!
//! impl Service<Uri> for MyConnector {
//!     type Response = MyConnection;
//!     type Error = BoxError;
//!     type Future = Pin<Box<dyn Future<Output = Result<MyConnection, BoxError>> + Send>>;
//!
//!     fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//!         Poll::Ready(Ok(()))
//!     }
//!
//!     fn call(&mut self, uri: Uri) -> Self::Future {
//!         Box::pin(async move {
//!             // Connect to uri.host():uri.port()
//!             // Return your connection
//!             todo!()
//!         })
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = reqwest::Client::builder()
//!     .custom_connector(MyConnector { /* ... */ })
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use std::error::Error as StdError;

/// A boxed error type for connectors.
pub type BoxError = Box<dyn StdError + Send + Sync>;

// Re-export hyper types needed for implementing connectors
pub use hyper::rt::{Read, Write};
pub use hyper_util::client::legacy::connect::{Connected, Connection};
