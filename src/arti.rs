use std::{
    future::Future,
    mem::MaybeUninit,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll},
};

use arti_client::{DataStream, IntoTorAddr, TorClient};
use http::{uri::Scheme, Uri};
#[cfg(any(feature = "default-tls", feature = "__rustls"))]
use hyper::rt::Write;
use hyper_util::client::legacy::connect::{Connected, Connection};
#[cfg(any(feature = "default-tls", feature = "__rustls"))]
use hyper_util::rt::TokioIo;
use pin_project::pin_project;
use thiserror::Error;
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

#[derive(Clone)]
pub(crate) enum Tls {
    #[cfg(not(feature = "__tls"))]
    Http,
    #[cfg(feature = "default-tls")]
    DefaultTls(native_tls_crate::TlsConnector),
    #[cfg(feature = "__rustls")]
    RustlsTls { tls: Arc<rustls::ClientConfig> },
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
    TLS(#[from] Arc<anyhow::Error>),
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

impl<R: Runtime> Service<Uri> for ArtiHttpConnector<R> {
    type Response = ArtiHttpConnection;
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
            let (host, port, use_tls) = uri_to_host_port_tls(req.clone())?;
            // Initiate a new Tor connection, producing a `DataStream` if successful.
            let addr = (&host as &str, port)
                .into_tor_addr()
                .map_err(arti_client::Error::from)?;
            let ds = client.connect(addr).await?;

            let inner = match use_tls {
                UseTls::Tls => match &*tls_conn {
                    #[cfg(feature = "default-tls")]
                    Tls::DefaultTls(tls_connector) => {
                        use crate::connect::native_tls_conn::NativeTlsConn;

                        let tls_connector =
                            tokio_native_tls::TlsConnector::from(tls_connector.clone());

                        let connect = tls_connector.connect(&host, ds).await.unwrap();

                        let v: NativeTlsConn<DataStream> = NativeTlsConn {
                            inner: TokioIo::new(connect),
                        };
                        MaybeHttpsStream::NativeHttps(v)
                    }
                    #[cfg(not(feature = "__tls"))]
                    Tls::Http => MaybeHttpsStream::Http(Box::new(ds).into()),
                    #[cfg(feature = "__rustls")]
                    Tls::RustlsTls { tls, .. } => {
                        use crate::connect::rustls_tls_conn::RustlsTlsConn;
                        use anyhow::anyhow;
                        use tokio_rustls::TlsConnector as RustlsConnector;

                        let tls = tls.clone();
                        let server_name =
                            rustls_pki_types::ServerName::try_from(host.as_str().to_owned())
                                .map_err(|_| ConnectionError::UnsupportedUriScheme { uri: req })?;
                        let io = RustlsConnector::from(tls)
                            .connect(server_name, ds)
                            .await
                            .map_err(|e| Arc::new(anyhow!(e)))
                            .map_err(ConnectionError::TLS)?;
                        let v: RustlsTlsConn<DataStream> = RustlsTlsConn {
                            inner: TokioIo::new(io),
                        };
                        MaybeHttpsStream::RustLsHttps(v)
                    }
                },
                UseTls::Bare => MaybeHttpsStream::Http(Box::new(ds).into()),
            };

            Ok(ArtiHttpConnection { inner })
        })
    }
}

pub struct ArtiHttpConnector<R: Runtime> {
    /// The client
    client: Arc<TorClient<R>>,

    /// TLS for using across Tor.
    tls_conn: Arc<Tls>,
}

impl<R: Runtime> Clone for ArtiHttpConnector<R> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            tls_conn: self.tls_conn.clone(),
        }
    }
}

impl<R: Runtime> ArtiHttpConnector<R> {
    /// Make a new `ArtiHttpConnector` using an Arti `TorClient` object.
    pub fn new(client: TorClient<R>, tls_conn: Tls) -> Self {
        let tls_conn = tls_conn.into();
        Self {
            client: Arc::new(client),
            tls_conn,
        }
    }
}

#[pin_project]
pub struct ArtiHttpConnection {
    /// The stream
    #[pin]
    inner: MaybeHttpsStream,
}

/// The actual actual stream; might be TLS, might not
#[pin_project(project = MaybeHttpsStreamProj)]
enum MaybeHttpsStream {
    /// http
    Http(Pin<Box<DataStream>>), // Tc:TlsStream is generally boxed; box this one too

    #[cfg(feature = "default-tls")]
    /// https
    NativeHttps(#[pin] crate::connect::native_tls_conn::NativeTlsConn<DataStream>),
    #[cfg(feature = "__rustls")]
    /// https
    RustLsHttps(#[pin] crate::connect::rustls_tls_conn::RustlsTlsConn<DataStream>),
}

impl Connection for ArtiHttpConnection {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

impl AsyncWrite for ArtiHttpConnection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_write(cx, buf),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => {
                let inner = t.project().inner;
                inner.poll_write(cx, buf)
            }
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => {
                let inner = t.project().inner;
                inner.poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_flush(cx),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => t.project().inner.poll_flush(cx),
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => t.project().inner.poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_shutdown(cx),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => t.project().inner.poll_shutdown(cx),
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => t.project().inner.poll_shutdown(cx),
        }
    }
}

impl hyper::rt::Read for ArtiHttpConnection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => {
                let data: &mut [MaybeUninit<u8>] = unsafe { buf.as_mut() };
                let mut read_buf = ReadBuf::uninit(data);
                let inner_read = ds.as_mut().poll_read(cx, &mut read_buf);
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
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => t.project().inner.poll_read(cx, buf),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => t.project().inner.poll_read(cx, buf),
        }
    }
}

/// [`WithHyperIo<I>`] is [`Write`] if `I` is [`AsyncWrite`].
///
/// [`AsyncWrite`]: tokio::io::AsyncWrite
/// [`Write`]: hyper::rt::Write
impl hyper::rt::Write for ArtiHttpConnection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_write(cx, buf),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => t.project().inner.poll_write(cx, buf),
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => t.project().inner.poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_flush(cx),
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => t.project().inner.poll_flush(cx),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => t.project().inner.poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_shutdown(cx),
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => t.project().inner.poll_shutdown(cx),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => t.project().inner.poll_shutdown(cx),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match &self.inner {
            MaybeHttpsStream::Http(ds) => ds.is_write_vectored(),
            #[cfg(feature = "__rustls")]
            MaybeHttpsStream::RustLsHttps(t) => t.inner.is_write_vectored(),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStream::NativeHttps(t) => t.inner.is_write_vectored(),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.project().inner.project() {
            MaybeHttpsStreamProj::Http(ds) => ds.as_mut().poll_write_vectored(cx, bufs),
            #[cfg(feature = "__rustls")]
            MaybeHttpsStreamProj::RustLsHttps(t) => t.project().inner.poll_write_vectored(cx, bufs),
            #[cfg(feature = "default-tls")]
            MaybeHttpsStreamProj::NativeHttps(t) => t.project().inner.poll_write_vectored(cx, bufs),
        }
    }
}
