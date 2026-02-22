#![deny(warnings)]

//! Example demonstrating how to use a custom connector with reqwest.
//!
//! This example shows how to implement a custom transport layer that can be
//! used with reqwest's `ClientBuilder::custom_connector()` method.
//!
//! Use cases for custom connectors include:
//! - WireGuard or other VPN tunnels
//! - Custom TLS implementations
//! - Network virtualization (e.g., smoltcp)
//! - Connection interception/modification
//!
//! Run with: `cargo run --example custom_connector --features custom-hyper-connector,rustls-tls`

use http::Uri;
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use std::future::Future;
use std::io::{self, IoSlice};
use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tower_service::Service;

/// A custom connector that wraps standard TCP/TLS connections.
///
/// This example connector demonstrates:
/// - How to implement `Service<Uri>` for a connector
/// - How to handle both HTTP and HTTPS connections
/// - How to implement the required hyper I/O traits
///
/// In a real-world scenario, you might replace the TCP connection with
/// your own transport (e.g., WireGuard tunnel, custom network stack).
#[derive(Clone)]
pub struct MyConnector {
    tls_connector: TlsConnector,
    /// Track number of connections (just for demonstration)
    connection_count: Arc<AtomicUsize>,
}

impl MyConnector {
    pub fn new() -> Self {
        // Set up rustls with webpki roots
        let root_store =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let tls_connector = TlsConnector::from(Arc::new(tls_config));

        Self {
            tls_connector,
            connection_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Service<Uri> for MyConnector {
    type Response = MyStream;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let tls_connector = self.tls_connector.clone();
        let count = self.connection_count.fetch_add(1, Ordering::SeqCst) + 1;

        Box::pin(async move {
            let host = uri
                .host()
                .ok_or_else(|| format!("URI has no host: {}", uri))?;

            let is_https = uri.scheme_str() == Some("https");
            let port = uri.port_u16().unwrap_or(if is_https { 443 } else { 80 });

            println!("[Connection #{}] Connecting to {}:{}", count, host, port);

            // Resolve hostname to IP address
            let addr = format!("{}:{}", host, port)
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| format!("Failed to resolve host: {}", host))?;

            // Create TCP connection
            // In a real custom connector, you might use your own transport here
            // (e.g., WireGuard tunnel, smoltcp network stack, etc.)
            let tcp_stream = TcpStream::connect(addr).await?;
            println!("[Connection #{}] TCP connected to {}", count, addr);

            if is_https {
                // Wrap with TLS
                let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
                    .map_err(|e| format!("Invalid server name: {}", e))?;

                println!("[Connection #{}] Starting TLS handshake", count);
                let tls_stream = tls_connector.connect(server_name, tcp_stream).await?;
                println!("[Connection #{}] TLS handshake completed", count);

                Ok(MyStream::Tls(Box::new(tls_stream)))
            } else {
                Ok(MyStream::Plain(tcp_stream))
            }
        })
    }
}

/// A stream that can be either plain TCP or TLS-wrapped.
///
/// This enum allows the connector to handle both HTTP and HTTPS connections.
pub enum MyStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

// Implement hyper_util's Connection trait
impl Connection for MyStream {
    fn connected(&self) -> Connected {
        // You can provide additional connection metadata here
        // For example, whether the connection is proxied, ALPN protocol, etc.
        Connected::new()
    }
}

// ============================================================================
// I/O trait implementations
//
// These implementations bridge between tokio's AsyncRead/AsyncWrite traits
// and hyper's Read/Write traits.
// ============================================================================

impl AsyncRead for MyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            MyStream::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            MyStream::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for MyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            MyStream::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            MyStream::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            MyStream::Plain(stream) => Pin::new(stream).poll_flush(cx),
            MyStream::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            MyStream::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            MyStream::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

// Implement hyper's Read trait for MyStream
impl Read for MyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        // Safety: We need to convert between hyper's ReadBufCursor and tokio's ReadBuf
        let mut temp_buf = vec![0u8; unsafe { buf.as_mut().len() }];
        let mut read_buf = ReadBuf::new(&mut temp_buf);

        match <Self as AsyncRead>::poll_read(self, cx, &mut read_buf) {
            Poll::Ready(Ok(())) => {
                let filled = read_buf.filled();
                if !filled.is_empty() {
                    unsafe {
                        let unfilled = buf.as_mut();
                        for (i, byte) in filled.iter().enumerate() {
                            unfilled[i].write(*byte);
                        }
                        buf.advance(filled.len());
                    }
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

// Implement hyper's Write trait for MyStream
impl Write for MyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        <Self as AsyncWrite>::poll_write(self, cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            MyStream::Plain(stream) => Pin::new(stream).poll_write_vectored(cx, bufs),
            MyStream::Tls(stream) => Pin::new(stream.as_mut()).poll_write_vectored(cx, bufs),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        <Self as AsyncWrite>::poll_flush(self, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        <Self as AsyncWrite>::poll_shutdown(self, cx)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create our custom connector
    let connector = MyConnector::new();

    // Build the reqwest client with our custom connector
    let client = reqwest::Client::builder()
        .custom_connector(connector)
        .build()?;

    println!("Making request using custom connector...\n");

    // Make a request - it will use our custom connector
    let response = client.get("https://httpbin.org/get").send().await?;

    println!("\nResponse status: {}", response.status());
    println!("Response headers: {:#?}", response.headers());

    let body = response.text().await?;
    println!("\nResponse body:\n{}", body);

    Ok(())
}
