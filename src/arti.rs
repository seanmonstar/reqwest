use std::{
    future::Future,
    mem::MaybeUninit,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use arti_client::{DataStream, IntoTorAddr, TorClient};
use http::{uri::Scheme, Uri};
use hyper_util::client::legacy::connect::{Connected, Connection};
use pin_project::pin_project;
use thiserror::Error;
use tls_api::TlsConnector as TlsConn;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tor_rtcompat::Runtime;
use tower_service::Service;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
/// Are we doing TLS?
enum UseTls {
    /// No
    Bare,

    /// Yes
    Tls,
}

#[derive(Error, Clone, Debug)]
#[non_exhaustive]
pub enum ConnectionError {
    /// Unsupported URI scheme
    #[error("unsupported URI scheme in {uri:?}")]
    UnsupportedUriScheme {
        /// URI
        uri: Uri,
    },

    /// Missing hostname
    #[error("Missing hostname in {uri:?}")]
    MissingHostname {
        /// URI
        uri: Uri,
    },

    /// Tor connection failed
    #[error("Tor connection failed")]
    Arti(#[from] arti_client::Error),

    /// TLS connection failed
    #[error("TLS connection failed")]
    TLS(#[source] Arc<anyhow::Error>),
}

/// Convert uri to http\[s\] host and port, and whether to do tls
fn uri_to_host_port_tls(uri: Uri) -> Result<(String, u16, UseTls), ConnectionError> {
    let use_tls = {
        // Scheme doesn't derive PartialEq so can't be matched on
        let scheme = uri.scheme();
        if scheme == Some(&Scheme::HTTP) {
            UseTls::Bare
        } else if scheme == Some(&Scheme::HTTPS) {
            UseTls::Tls
        } else {
            return Err(ConnectionError::UnsupportedUriScheme { uri });
        }
    };
    let host = match uri.host() {
        Some(h) => h,
        _ => return Err(ConnectionError::MissingHostname { uri }),
    };
    let port = uri.port().map(|x| x.as_u16()).unwrap_or(match use_tls {
        UseTls::Tls => 443,
        UseTls::Bare => 80,
    });

    Ok((host.to_owned(), port, use_tls))
}

impl<R: Runtime, TC: TlsConn> Service<Uri> for ArtiHttpConnector<R, TC> {
    type Response = ArtiHttpConnection<TC>;
    type Error = ConnectionError;
    #[allow(clippy::type_complexity)]
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        // `TorClient` objects can be cloned cheaply (the cloned objects refer to the same
        // underlying handles required to make Tor connections internally).
        // We use this to avoid the returned future having to borrow `self`.
        let client = self.client.clone();
        let tls_conn = self.tls_conn.clone();
        Box::pin(async move {
            // Extract the host and port to connect to from the URI.
            let (host, port, use_tls) = uri_to_host_port_tls(req)?;
            // Initiate a new Tor connection, producing a `DataStream` if successful.
            let addr = (&host as &str, port)
                .into_tor_addr()
                .map_err(arti_client::Error::from)?;
            let ds = client.connect(addr).await?;

            let inner = match use_tls {
                UseTls::Tls => {
                    let conn = tls_conn
                        .connect_impl_tls_stream(&host, ds)
                        .await
                        .map_err(|e| ConnectionError::TLS(e.into()))?;
                    MaybeHttpsStream::Https(conn)
                }
                UseTls::Bare => MaybeHttpsStream::Http(Box::new(ds).into()),
            };

            Ok(ArtiHttpConnection { inner })
        })
    }
}

pub struct ArtiHttpConnector<R: Runtime, TC: TlsConn> {
    /// The client
    client: Arc<TorClient<R>>,

    /// TLS for using across Tor.
    tls_conn: Arc<TC>,
}

impl<R: Runtime, TC: TlsConn> Clone for ArtiHttpConnector<R, TC> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            tls_conn: self.tls_conn.clone(),
        }
    }
}

impl<R: Runtime, TC: TlsConn> ArtiHttpConnector<R, TC> {
    /// Make a new `ArtiHttpConnector` using an Arti `TorClient` object.
    pub fn new(client: TorClient<R>, tls_conn: TC) -> Self {
        let tls_conn = tls_conn.into();
        Self {
            client: Arc::new(client),
            tls_conn,
        }
    }
}

#[pin_project]
pub struct ArtiHttpConnection<TC: TlsConn> {
    /// The stream
    #[pin]
    inner: MaybeHttpsStream<TC>,
}

/// The actual actual stream; might be TLS, might not
#[pin_project(project = MaybeHttpsStreamProj)]
enum MaybeHttpsStream<TC: TlsConn> {
    /// http
    Http(Pin<Box<DataStream>>), // Tc:TlsStream is generally boxed; box this one too

    /// https
    Https(#[pin] TC::TlsStream),
}

impl<TC: TlsConn> Connection for ArtiHttpConnection<TC> {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

// These trait implementations just defer to the inner `DataStream`; the wrapper type is just
// there to implement the `Connection` trait.
impl<TC: TlsConn> AsyncRead for ArtiHttpConnection<TC> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_read(cx, buf),
            MaybeHttpsStreamProj::Https(t) => t.poll_read(cx, buf),
        }
    }
}

impl<TC: TlsConn> AsyncWrite for ArtiHttpConnection<TC> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_write(cx, buf),
            MaybeHttpsStreamProj::Https(t) => t.poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_flush(cx),
            MaybeHttpsStreamProj::Https(t) => t.poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_shutdown(cx),
            MaybeHttpsStreamProj::Https(t) => t.poll_shutdown(cx),
        }
    }
}

impl<TC: TlsConn> hyper::rt::Read for ArtiHttpConnection<TC> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let data: &mut [MaybeUninit<u8>] = unsafe { buf.as_mut() };
        let mut read_buf = ReadBuf::uninit(data);
        let inner_read = match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_read(cx, &mut read_buf),
            MaybeHttpsStreamProj::Https(t) => t.poll_read(cx, &mut read_buf),
        };
        match inner_read {
            Poll::Ready(Ok(())) => {
                let n = read_buf.filled().len();
                unsafe {
                    buf.advance(n);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// [`WithHyperIo<I>`] is [`Write`] if `I` is [`AsyncWrite`].
///
/// [`AsyncWrite`]: tokio::io::AsyncWrite
/// [`Write`]: hyper::rt::Write
impl<TC: TlsConn> hyper::rt::Write for ArtiHttpConnection<TC> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_write(cx, buf),
            MaybeHttpsStreamProj::Https(t) => t.poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_flush(cx),
            MaybeHttpsStreamProj::Https(t) => t.poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_shutdown(cx),
            MaybeHttpsStreamProj::Https(t) => t.poll_shutdown(cx),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match &self.inner {
            MaybeHttpsStream::Http(ds) => ds.is_write_vectored(),
            MaybeHttpsStream::Https(t) => t.is_write_vectored(),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_write_vectored(cx, bufs),
            MaybeHttpsStreamProj::Https(t) => t.poll_write_vectored(cx, bufs),
        }
    }
}
