use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::Uri;
use shadowsocks::{
    config::{ServerConfig, ServerType},
    relay::Address,
    context::Context as SsContext,
    ProxyClientStream,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use url::Url;

use crate::{
    connect::{BoxError, Conn},
    proxy::Intercepted,
};

#[cfg(feature = "default-tls")]
use native_tls_crate as native_tls;

pub struct UnpinProxyClientStream(pub Box<shadowsocks::ProxyClientStream<shadowsocks::net::TcpStream>>);

impl UnpinProxyClientStream {
    fn new(stream: shadowsocks::ProxyClientStream<shadowsocks::net::TcpStream>) -> Self {
        Self(Box::new(stream))
    }
}

impl AsyncRead for UnpinProxyClientStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for UnpinProxyClientStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.0).poll_shutdown(cx)
    }
}

impl hyper::rt::Read for UnpinProxyClientStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let n = unsafe {
            let mut tbuf = ReadBuf::uninit(buf.as_mut());
            match Pin::new(&mut *self.0).poll_read(cx, &mut tbuf) {
                Poll::Ready(Ok(())) => tbuf.filled().len(),
                other => return other,
            }
        };

        unsafe {
            buf.advance(n);
        }
        Poll::Ready(Ok(()))
    }
}

impl hyper::rt::Write for UnpinProxyClientStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut *self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut *self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut *self.0).poll_shutdown(cx)
    }
}

impl hyper_util::client::legacy::connect::Connection for UnpinProxyClientStream {
    fn connected(&self) -> hyper_util::client::legacy::connect::Connected {
        hyper_util::client::legacy::connect::Connected::new()
    }
}

impl crate::connect::TlsInfoFactory for UnpinProxyClientStream {
    fn tls_info(&self) -> Option<crate::tls::TlsInfo> {
        None
    }
}

#[cfg(feature = "default-tls")]
impl crate::connect::TlsInfoFactory for tokio_native_tls::TlsStream<hyper_util::rt::TokioIo<UnpinProxyClientStream>> {
    fn tls_info(&self) -> Option<crate::tls::TlsInfo> {
        let peer_certificate = self
            .get_ref()
            .peer_certificate()
            .ok()
            .flatten()
            .and_then(|c| c.to_der().ok());
        Some(crate::tls::TlsInfo { peer_certificate })
    }
}

#[cfg(feature = "default-tls")]
impl hyper_util::client::legacy::connect::Connection for crate::connect::native_tls_conn::NativeTlsConn<hyper_util::rt::TokioIo<UnpinProxyClientStream>> {
    fn connected(&self) -> hyper_util::client::legacy::connect::Connected {
        hyper_util::client::legacy::connect::Connected::new()
    }
}

pub(super) async fn connect_ss(
    connector: super::ConnectorService,
    dst: Uri,
    proxy: Intercepted,
) -> Result<Conn, BoxError> {


    // Check for the original Shadowsocks URL
    let proxy_url = if let Some(ss) = proxy.ss() {
        ss.clone()
    } else {
        Url::parse(&proxy.uri().to_string())?
    };

    let sc = ServerConfig::from_url(proxy_url.as_str())?;

    let host = dst.host().ok_or("no host in url")?.to_string();
    let port = dst
        .port_u16()
        .unwrap_or_else(|| if dst.scheme() == Some(&http::uri::Scheme::HTTPS) {
            443
        } else {
            80
        });
    let target_addr = Address::from((host.clone(), port));

    let ctx = Arc::new(SsContext::new(ServerType::Local));
    let stream = ProxyClientStream::connect(ctx, &sc, &target_addr).await?;
    let stream = UnpinProxyClientStream::new(stream);
    let io = hyper_util::rt::TokioIo::new(stream);

    if dst.scheme() == Some(&http::uri::Scheme::HTTPS) {
        match &connector.inner {
            #[cfg(feature = "default-tls")]
            super::Inner::DefaultTls(_, _) => {
                let mut builder = native_tls::TlsConnector::builder();
                // TODO: The `danger_accept_invalid_certs` setting is not propagated to this point
                // for proxy-over-TLS scenarios with `native-tls`. This is a limitation in the
                // current architecture and is hardcoded to `true` for now to make it work.
                // A proper fix would involve a deeper refactoring to plumb the setting down.
                builder.danger_accept_invalid_certs(true);

                let tls_connector = tokio_native_tls::TlsConnector::from(
                    builder.build().map_err(|e| Box::new(e) as BoxError)?,
                );

                let stream = match tls_connector.connect(&host, io).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        log::error!("TLS connection failed: {}", e);
                        return Err(Box::new(e));
                    }
                };
                let conn = super::sealed::Conn {
                    inner: connector.verbose.wrap(super::native_tls_conn::NativeTlsConn { inner: hyper_util::rt::TokioIo::new(stream) }),
                    is_proxy: false,
                    tls_info: connector.tls_info,
                };
                return Ok(conn);
            }            #[cfg(feature = "__rustls")]
            super::Inner::RustlsTls { tls, .. } => {
                use std::convert::TryFrom;
                let server_name = match host.parse::<std::net::IpAddr>() {
                    Ok(ip) => rustls_pki_types::ServerName::IpAddress(ip.into()),
                    Err(_) => rustls_pki_types::ServerName::try_from(host.as_str())
                        .map_err(|e| Box::new(e) as BoxError)?
                        .to_owned(),
                };
                use tokio_rustls::TlsConnector as RustlsConnector;
                let stream = match RustlsConnector::from(tls.clone()).connect(server_name, io).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        log::error!("TLS connection failed: {}", e);
                        return Err(Box::new(e));
                    }
                };
                let conn = super::sealed::Conn {
                    inner: connector.verbose.wrap(super::rustls_tls_conn::RustlsTlsConn { inner: hyper_util::rt::TokioIo::new(stream) }),
                    is_proxy: false,
                    tls_info: connector.tls_info,
                };
                return Ok(conn);
            }
        }
    }

    let conn = super::sealed::Conn {
        inner: connector.verbose.wrap(io),
        is_proxy: false,
        tls_info: false,
    };
    Ok(conn)
}

#[cfg(all(feature = "__rustls", not(target_arch = "wasm32")))]
impl crate::connect::TlsInfoFactory for tokio_rustls::client::TlsStream<hyper_util::rt::TokioIo<UnpinProxyClientStream>> {
    fn tls_info(&self) -> Option<crate::tls::TlsInfo> {
        let (_, session) = self.get_ref();
        let peer_certificate = session
            .peer_certificates()
            .and_then(|certs| certs.first().map(|c| c.to_vec()));
        Some(crate::tls::TlsInfo { peer_certificate })
    }
}

#[cfg(all(feature = "__rustls", not(target_arch = "wasm32")))]
impl hyper_util::client::legacy::connect::Connection for crate::connect::rustls_tls_conn::RustlsTlsConn<hyper_util::rt::TokioIo<UnpinProxyClientStream>> {
    fn connected(&self) -> hyper_util::client::legacy::connect::Connected {
        if self.inner.inner().get_ref().1.alpn_protocol() == Some(b"h2") {
            self.inner
                .inner()
                .get_ref()
                .0
                .inner()
                .connected()
                .negotiated_h2()
        } else {
            self.inner.inner().get_ref().0.inner().connected()
        }
    }
}