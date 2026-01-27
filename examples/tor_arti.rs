#![deny(warnings)]

//! Example demonstrating how to use reqwest with the Arti Tor client.
//!
//! This example shows how to route HTTP requests through the Tor network
//! using Arti (the Rust implementation of Tor) as a custom connector.
//!
//! This provides anonymous browsing by routing all traffic through the
//! Tor network, hiding your IP address from the destination server.
//!
//! # Prerequisites
//!
//! This example requires the `arti-client` and `tor-rtcompat` crates.
//! The Arti client will automatically bootstrap and connect to the Tor
//! network on first use (this may take 30-60 seconds).
//!
//! # Run with:
//! ```bash
//! cargo run --example tor_arti --no-default-features --features custom-hyper-connector,rustls-tls
//! ```
//!
//! # Note
//!
//! The first run will take some time as Arti downloads the Tor network
//! consensus and establishes circuits. Subsequent runs will be faster
//! as the data is cached.

use arti_client::{TorClient, TorClientConfig};
use http::Uri;
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use std::future::Future;
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tor_rtcompat::PreferredRuntime;
use tower_service::Service;

/// A connector that routes connections through the Tor network using Arti.
///
/// This connector:
/// 1. Uses Arti to create anonymous TCP connections through Tor
/// 2. Wraps HTTPS connections with TLS (Arti provides the TCP layer)
/// 3. Provides end-to-end encryption for HTTPS sites
#[derive(Clone)]
pub struct ArtiConnector {
    /// The Arti Tor client
    tor_client: TorClient<PreferredRuntime>,
    /// TLS connector for HTTPS connections
    tls_connector: TlsConnector,
}

impl ArtiConnector {
    /// Create a new Arti connector.
    ///
    /// This will bootstrap the Tor client if not already connected.
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        println!("Initializing Arti Tor client...");
        println!("(This may take 30-60 seconds on first run while downloading Tor consensus)");

        // Create default Tor client configuration
        let config = TorClientConfig::default();

        // Bootstrap the Tor client
        let tor_client = TorClient::create_bootstrapped(config).await?;

        println!("Tor client bootstrapped successfully!");

        // Set up rustls with webpki roots for TLS
        let root_store =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let tls_connector = TlsConnector::from(Arc::new(tls_config));

        Ok(Self {
            tor_client,
            tls_connector,
        })
    }
}

impl Service<Uri> for ArtiConnector {
    type Response = ArtiStream;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        let tor_client = self.tor_client.clone();
        let tls_connector = self.tls_connector.clone();

        Box::pin(async move {
            let host = uri
                .host()
                .ok_or_else(|| format!("URI has no host: {}", uri))?;

            let is_https = uri.scheme_str() == Some("https");
            let port = uri.port_u16().unwrap_or(if is_https { 443 } else { 80 });

            println!("[Tor] Connecting to {}:{} through Tor network...", host, port);

            // Create an anonymous TCP connection through Tor
            // Arti handles DNS resolution through Tor as well (no DNS leaks!)
            let tor_stream = tor_client
                .connect((host, port))
                .await
                .map_err(|e| format!("Tor connection failed: {}", e))?;

            println!("[Tor] Circuit established to {}:{}", host, port);

            if is_https {
                // Wrap the Tor stream with TLS for HTTPS
                let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
                    .map_err(|e| format!("Invalid server name: {}", e))?;

                println!("[Tor] Starting TLS handshake over Tor...");
                let tls_stream = tls_connector
                    .connect(server_name, tor_stream)
                    .await
                    .map_err(|e| format!("TLS handshake over Tor failed: {}", e))?;

                println!("[Tor] TLS handshake completed");
                Ok(ArtiStream::Tls(Box::new(tls_stream)))
            } else {
                Ok(ArtiStream::Plain(tor_stream))
            }
        })
    }
}

/// A stream that wraps Arti's DataStream, optionally with TLS.
pub enum ArtiStream {
    /// Plain TCP over Tor (for HTTP)
    Plain(arti_client::DataStream),
    /// TLS over Tor (for HTTPS)
    Tls(Box<TlsStream<arti_client::DataStream>>),
}

// Implement hyper_util's Connection trait
impl Connection for ArtiStream {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

// ============================================================================
// I/O trait implementations
// ============================================================================

impl AsyncRead for ArtiStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ArtiStream::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            ArtiStream::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ArtiStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            ArtiStream::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            ArtiStream::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ArtiStream::Plain(stream) => Pin::new(stream).poll_flush(cx),
            ArtiStream::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ArtiStream::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            ArtiStream::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

// Implement hyper's Read trait
impl Read for ArtiStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
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

// Implement hyper's Write trait
impl Write for ArtiStream {
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
            ArtiStream::Plain(stream) => Pin::new(stream).poll_write_vectored(cx, bufs),
            ArtiStream::Tls(stream) => Pin::new(stream.as_mut()).poll_write_vectored(cx, bufs),
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
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize the Arti Tor connector
    let connector = ArtiConnector::new().await?;

    // Build the reqwest client with the Tor connector
    let client = reqwest::Client::builder()
        .custom_connector(connector)
        .build()?;

    println!("\nMaking request through Tor network...\n");

    // Check our IP through Tor - this should show a Tor exit node IP
    let response = client
        .get("https://check.torproject.org/api/ip")
        .send()
        .await?;

    println!("Response status: {}", response.status());

    let body = response.text().await?;
    println!("Response (your IP through Tor): {}", body);

    // Verify we're using Tor
    println!("\nVerifying Tor connection...");
    let response = client
        .get("https://check.torproject.org")
        .send()
        .await?;

    let html = response.text().await?;
    if html.contains("Congratulations") {
        println!("Success! You are using Tor.");
    } else {
        println!("Warning: Tor verification unclear.");
    }

    Ok(())
}
