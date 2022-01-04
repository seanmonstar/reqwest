use futures_util::future::Either;
#[cfg(feature = "__tls")]
use http::header::HeaderValue;
use http::uri::{Authority, Scheme};
use http::Uri;
use hyper::client::connect::{
    dns::{GaiResolver, Name},
    Connected, Connection,
};
use hyper::service::Service;
#[cfg(feature = "native-tls-crate")]
use native_tls_crate::{TlsConnector, TlsConnectorBuilder};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use pin_project_lite::pin_project;
use std::io::IoSlice;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{collections::HashMap, io};
use std::{future::Future, net::SocketAddr};

#[cfg(feature = "default-tls")]
use self::native_tls_conn::NativeTlsConn;
#[cfg(feature = "__rustls")]
use self::rustls_tls_conn::RustlsTlsConn;
#[cfg(feature = "trust-dns")]
use crate::dns::TrustDnsResolver;
use crate::error::BoxError;
use crate::proxy::{Proxy, ProxyScheme};

#[derive(Clone)]
pub(crate) enum HttpConnector {
    Gai(hyper::client::HttpConnector),
    GaiWithDnsOverrides(hyper::client::HttpConnector<DnsResolverWithOverrides<GaiResolver>>),
    #[cfg(feature = "trust-dns")]
    TrustDns(hyper::client::HttpConnector<TrustDnsResolver>),
    #[cfg(feature = "trust-dns")]
    TrustDnsWithOverrides(hyper::client::HttpConnector<DnsResolverWithOverrides<TrustDnsResolver>>),
}

impl HttpConnector {
    pub(crate) fn new_gai() -> Self {
        Self::Gai(hyper::client::HttpConnector::new())
    }

    pub(crate) fn new_gai_with_overrides(overrides: HashMap<String, SocketAddr>) -> Self {
        let gai = hyper::client::connect::dns::GaiResolver::new();
        let overridden_resolver = DnsResolverWithOverrides::new(gai, overrides);
        Self::GaiWithDnsOverrides(hyper::client::HttpConnector::new_with_resolver(
            overridden_resolver,
        ))
    }

    #[cfg(feature = "trust-dns")]
    pub(crate) fn new_trust_dns() -> crate::Result<HttpConnector> {
        TrustDnsResolver::new()
            .map(hyper::client::HttpConnector::new_with_resolver)
            .map(Self::TrustDns)
            .map_err(crate::error::builder)
    }

    #[cfg(feature = "trust-dns")]
    pub(crate) fn new_trust_dns_with_overrides(
        overrides: HashMap<String, SocketAddr>,
    ) -> crate::Result<HttpConnector> {
        TrustDnsResolver::new()
            .map(|resolver| DnsResolverWithOverrides::new(resolver, overrides))
            .map(hyper::client::HttpConnector::new_with_resolver)
            .map(Self::TrustDnsWithOverrides)
            .map_err(crate::error::builder)
    }
}

macro_rules! impl_http_connector {
    ($(fn $name:ident(&mut self, $($par_name:ident: $par_type:ty),*)$( -> $return:ty)?;)+) => {
        #[allow(dead_code)]
        impl HttpConnector {
            $(
                fn $name(&mut self, $($par_name: $par_type),*)$( -> $return)? {
                    match self {
                        Self::Gai(resolver) => resolver.$name($($par_name),*),
                        Self::GaiWithDnsOverrides(resolver) => resolver.$name($($par_name),*),
                        #[cfg(feature = "trust-dns")]
                        Self::TrustDns(resolver) => resolver.$name($($par_name),*),
                        #[cfg(feature = "trust-dns")]
                        Self::TrustDnsWithOverrides(resolver) => resolver.$name($($par_name),*),
                    }
                }
            )+
        }
    };
}

impl_http_connector! {
    fn set_local_address(&mut self, addr: Option<IpAddr>);
    fn enforce_http(&mut self, is_enforced: bool);
    fn set_nodelay(&mut self, nodelay: bool);
    fn set_keepalive(&mut self, dur: Option<Duration>);
}

impl Service<Uri> for HttpConnector {
    type Response = <hyper::client::HttpConnector as Service<Uri>>::Response;
    type Error = <hyper::client::HttpConnector as Service<Uri>>::Error;
    #[cfg(feature = "trust-dns")]
    type Future =
        Either<
            Either<
                <hyper::client::HttpConnector as Service<Uri>>::Future,
                <hyper::client::HttpConnector<DnsResolverWithOverrides<GaiResolver>> as Service<
                    Uri,
                >>::Future,
            >,
            Either<
                    <hyper::client::HttpConnector<TrustDnsResolver> as Service<Uri>>::Future,
                <hyper::client::HttpConnector<DnsResolverWithOverrides<TrustDnsResolver>> as Service<Uri>>::Future
                 >
        >;
    #[cfg(not(feature = "trust-dns"))]
    type Future =
        Either<
            Either<
                <hyper::client::HttpConnector as Service<Uri>>::Future,
                <hyper::client::HttpConnector<DnsResolverWithOverrides<GaiResolver>> as Service<
                    Uri,
                >>::Future,
            >,
            Either<
                <hyper::client::HttpConnector as Service<Uri>>::Future,
                <hyper::client::HttpConnector as Service<Uri>>::Future,
            >,
        >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self {
            Self::Gai(resolver) => resolver.poll_ready(cx),
            Self::GaiWithDnsOverrides(resolver) => resolver.poll_ready(cx),
            #[cfg(feature = "trust-dns")]
            Self::TrustDns(resolver) => resolver.poll_ready(cx),
            #[cfg(feature = "trust-dns")]
            Self::TrustDnsWithOverrides(resolver) => resolver.poll_ready(cx),
        }
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        match self {
            Self::Gai(resolver) => Either::Left(Either::Left(resolver.call(dst))),
            Self::GaiWithDnsOverrides(resolver) => Either::Left(Either::Right(resolver.call(dst))),
            #[cfg(feature = "trust-dns")]
            Self::TrustDns(resolver) => Either::Right(Either::Left(resolver.call(dst))),
            #[cfg(feature = "trust-dns")]
            Self::TrustDnsWithOverrides(resolver) => {
                Either::Right(Either::Right(resolver.call(dst)))
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Connector {
    inner: Inner,
    proxies: Arc<Vec<Proxy>>,
    verbose: verbose::Wrapper,
    timeout: Option<Duration>,
    #[cfg(feature = "__tls")]
    nodelay: bool,
    #[cfg(feature = "__tls")]
    user_agent: Option<HeaderValue>,
}

#[derive(Clone)]
enum Inner {
    #[cfg(not(feature = "__tls"))]
    Http(HttpConnector),
    #[cfg(feature = "default-tls")]
    DefaultTls(HttpConnector, TlsConnector),
    #[cfg(feature = "__rustls")]
    RustlsTls {
        http: HttpConnector,
        tls: Arc<rustls::ClientConfig>,
        tls_proxy: Arc<rustls::ClientConfig>,
    },
}

impl Connector {
    #[cfg(not(feature = "__tls"))]
    pub(crate) fn new<T>(
        mut http: HttpConnector,
        proxies: Arc<Vec<Proxy>>,
        local_addr: T,
        nodelay: bool,
    ) -> Connector
    where
        T: Into<Option<IpAddr>>,
    {
        http.set_local_address(local_addr.into());
        http.set_nodelay(nodelay);
        Connector {
            inner: Inner::Http(http),
            verbose: verbose::OFF,
            proxies,
            timeout: None,
        }
    }

    #[cfg(feature = "default-tls")]
    pub(crate) fn new_default_tls<T>(
        http: HttpConnector,
        tls: TlsConnectorBuilder,
        proxies: Arc<Vec<Proxy>>,
        user_agent: Option<HeaderValue>,
        local_addr: T,
        nodelay: bool,
    ) -> crate::Result<Connector>
    where
        T: Into<Option<IpAddr>>,
    {
        let tls = tls.build().map_err(crate::error::builder)?;
        Ok(Self::from_built_default_tls(
            http, tls, proxies, user_agent, local_addr, nodelay,
        ))
    }

    #[cfg(feature = "default-tls")]
    pub(crate) fn from_built_default_tls<T>(
        mut http: HttpConnector,
        tls: TlsConnector,
        proxies: Arc<Vec<Proxy>>,
        user_agent: Option<HeaderValue>,
        local_addr: T,
        nodelay: bool,
    ) -> Connector
    where
        T: Into<Option<IpAddr>>,
    {
        http.set_local_address(local_addr.into());
        http.enforce_http(false);

        Connector {
            inner: Inner::DefaultTls(http, tls),
            proxies,
            verbose: verbose::OFF,
            timeout: None,
            nodelay,
            user_agent,
        }
    }

    #[cfg(feature = "__rustls")]
    pub(crate) fn new_rustls_tls<T>(
        mut http: HttpConnector,
        tls: rustls::ClientConfig,
        proxies: Arc<Vec<Proxy>>,
        user_agent: Option<HeaderValue>,
        local_addr: T,
        nodelay: bool,
    ) -> Connector
    where
        T: Into<Option<IpAddr>>,
    {
        http.set_local_address(local_addr.into());
        http.enforce_http(false);

        let (tls, tls_proxy) = if proxies.is_empty() {
            let tls = Arc::new(tls);
            (tls.clone(), tls)
        } else {
            let mut tls_proxy = tls.clone();
            tls_proxy.alpn_protocols.clear();
            (Arc::new(tls), Arc::new(tls_proxy))
        };

        Connector {
            inner: Inner::RustlsTls {
                http,
                tls,
                tls_proxy,
            },
            proxies,
            verbose: verbose::OFF,
            timeout: None,
            nodelay,
            user_agent,
        }
    }

    pub(crate) fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    pub(crate) fn set_verbose(&mut self, enabled: bool) {
        self.verbose.0 = enabled;
    }

    #[cfg(feature = "socks")]
    async fn connect_socks(&self, dst: Uri, proxy: ProxyScheme) -> Result<Conn, BoxError> {
        let dns = match proxy {
            ProxyScheme::Socks5 {
                remote_dns: false, ..
            } => socks::DnsResolve::Local,
            ProxyScheme::Socks5 {
                remote_dns: true, ..
            } => socks::DnsResolve::Proxy,
            ProxyScheme::Http { .. } | ProxyScheme::Https { .. } => {
                unreachable!("connect_socks is only called for socks proxies");
            }
        };

        match &self.inner {
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(_http, tls) => {
                if dst.scheme() == Some(&Scheme::HTTPS) {
                    let host = dst.host().ok_or("no host in url")?.to_string();
                    let conn = socks::connect(proxy, dst, dns).await?;
                    let tls_connector = tokio_native_tls::TlsConnector::from(tls.clone());
                    let io = tls_connector.connect(&host, conn).await?;
                    return Ok(Conn {
                        inner: self.verbose.wrap(NativeTlsConn { inner: io }),
                        is_proxy: false,
                    });
                }
            }
            #[cfg(feature = "__rustls")]
            Inner::RustlsTls { tls_proxy, .. } => {
                if dst.scheme() == Some(&Scheme::HTTPS) {
                    use std::convert::TryFrom;
                    use tokio_rustls::TlsConnector as RustlsConnector;

                    let tls = tls_proxy.clone();
                    let host = dst.host().ok_or("no host in url")?.to_string();
                    let conn = socks::connect(proxy, dst, dns).await?;
                    let server_name = rustls::ServerName::try_from(host.as_str())
                        .map_err(|_| "Invalid Server Name")?;
                    let io = RustlsConnector::from(tls)
                        .connect(server_name, conn)
                        .await?;
                    return Ok(Conn {
                        inner: self.verbose.wrap(RustlsTlsConn { inner: io }),
                        is_proxy: false,
                    });
                }
            }
            #[cfg(not(feature = "__tls"))]
            Inner::Http(_) => (),
        }

        socks::connect(proxy, dst, dns).await.map(|tcp| Conn {
            inner: self.verbose.wrap(tcp),
            is_proxy: false,
        })
    }

    async fn connect_with_maybe_proxy(self, dst: Uri, is_proxy: bool) -> Result<Conn, BoxError> {
        match self.inner {
            #[cfg(not(feature = "__tls"))]
            Inner::Http(mut http) => {
                let io = http.call(dst).await?;
                Ok(Conn {
                    inner: self.verbose.wrap(io),
                    is_proxy,
                })
            }
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(http, tls) => {
                let mut http = http.clone();

                // Disable Nagle's algorithm for TLS handshake
                //
                // https://www.openssl.org/docs/man1.1.1/man3/SSL_connect.html#NOTES
                if !self.nodelay && (dst.scheme() == Some(&Scheme::HTTPS)) {
                    http.set_nodelay(true);
                }

                let tls_connector = tokio_native_tls::TlsConnector::from(tls.clone());
                let mut http = hyper_tls::HttpsConnector::from((http, tls_connector));
                let io = http.call(dst).await?;

                if let hyper_tls::MaybeHttpsStream::Https(stream) = io {
                    if !self.nodelay {
                        stream.get_ref().get_ref().get_ref().set_nodelay(false)?;
                    }
                    Ok(Conn {
                        inner: self.verbose.wrap(NativeTlsConn { inner: stream }),
                        is_proxy,
                    })
                } else {
                    Ok(Conn {
                        inner: self.verbose.wrap(io),
                        is_proxy,
                    })
                }
            }
            #[cfg(feature = "__rustls")]
            Inner::RustlsTls { http, tls, .. } => {
                let mut http = http.clone();

                // Disable Nagle's algorithm for TLS handshake
                //
                // https://www.openssl.org/docs/man1.1.1/man3/SSL_connect.html#NOTES
                if !self.nodelay && (dst.scheme() == Some(&Scheme::HTTPS)) {
                    http.set_nodelay(true);
                }

                let mut http = hyper_rustls::HttpsConnector::from((http, tls.clone()));
                let io = http.call(dst).await?;

                if let hyper_rustls::MaybeHttpsStream::Https(stream) = io {
                    if !self.nodelay {
                        let (io, _) = stream.get_ref();
                        io.set_nodelay(false)?;
                    }
                    Ok(Conn {
                        inner: self.verbose.wrap(RustlsTlsConn { inner: stream }),
                        is_proxy,
                    })
                } else {
                    Ok(Conn {
                        inner: self.verbose.wrap(io),
                        is_proxy,
                    })
                }
            }
        }
    }

    async fn connect_via_proxy(
        self,
        dst: Uri,
        proxy_scheme: ProxyScheme,
    ) -> Result<Conn, BoxError> {
        log::debug!("proxy({:?}) intercepts '{:?}'", proxy_scheme, dst);

        let (proxy_dst, _auth) = match proxy_scheme {
            ProxyScheme::Http { host, auth } => (into_uri(Scheme::HTTP, host), auth),
            ProxyScheme::Https { host, auth } => (into_uri(Scheme::HTTPS, host), auth),
            #[cfg(feature = "socks")]
            ProxyScheme::Socks5 { .. } => return self.connect_socks(dst, proxy_scheme).await,
        };

        #[cfg(feature = "__tls")]
        let auth = _auth;

        match &self.inner {
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(http, tls) => {
                if dst.scheme() == Some(&Scheme::HTTPS) {
                    let host = dst.host().to_owned();
                    let port = dst.port().map(|p| p.as_u16()).unwrap_or(443);
                    let http = http.clone();
                    let tls_connector = tokio_native_tls::TlsConnector::from(tls.clone());
                    let mut http = hyper_tls::HttpsConnector::from((http, tls_connector));
                    let conn = http.call(proxy_dst).await?;
                    log::trace!("tunneling HTTPS over proxy");
                    let tunneled = tunnel(
                        conn,
                        host.ok_or("no host in url")?.to_string(),
                        port,
                        self.user_agent.clone(),
                        auth,
                    )
                    .await?;
                    let tls_connector = tokio_native_tls::TlsConnector::from(tls.clone());
                    let io = tls_connector
                        .connect(host.ok_or("no host in url")?, tunneled)
                        .await?;
                    return Ok(Conn {
                        inner: self.verbose.wrap(NativeTlsConn { inner: io }),
                        is_proxy: false,
                    });
                }
            }
            #[cfg(feature = "__rustls")]
            Inner::RustlsTls {
                http,
                tls,
                tls_proxy,
            } => {
                if dst.scheme() == Some(&Scheme::HTTPS) {
                    use rustls::ServerName;
                    use std::convert::TryFrom;
                    use tokio_rustls::TlsConnector as RustlsConnector;

                    let host = dst.host().ok_or("no host in url")?.to_string();
                    let port = dst.port().map(|r| r.as_u16()).unwrap_or(443);
                    let http = http.clone();
                    let mut http = hyper_rustls::HttpsConnector::from((http, tls_proxy.clone()));
                    let tls = tls.clone();
                    let conn = http.call(proxy_dst).await?;
                    log::trace!("tunneling HTTPS over proxy");
                    let maybe_server_name =
                        ServerName::try_from(host.as_str()).map_err(|_| "Invalid Server Name");
                    let tunneled = tunnel(conn, host, port, self.user_agent.clone(), auth).await?;
                    let server_name = maybe_server_name?;
                    let io = RustlsConnector::from(tls)
                        .connect(server_name, tunneled)
                        .await?;

                    return Ok(Conn {
                        inner: self.verbose.wrap(RustlsTlsConn { inner: io }),
                        is_proxy: false,
                    });
                }
            }
            #[cfg(not(feature = "__tls"))]
            Inner::Http(_) => (),
        }

        self.connect_with_maybe_proxy(proxy_dst, true).await
    }

    pub fn set_keepalive(&mut self, dur: Option<Duration>) {
        match &mut self.inner {
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(http, _tls) => http.set_keepalive(dur),
            #[cfg(feature = "__rustls")]
            Inner::RustlsTls { http, .. } => http.set_keepalive(dur),
            #[cfg(not(feature = "__tls"))]
            Inner::Http(http) => http.set_keepalive(dur),
        }
    }
}

fn into_uri(scheme: Scheme, host: Authority) -> Uri {
    // TODO: Should the `http` crate get `From<(Scheme, Authority)> for Uri`?
    http::Uri::builder()
        .scheme(scheme)
        .authority(host)
        .path_and_query(http::uri::PathAndQuery::from_static("/"))
        .build()
        .expect("scheme and authority is valid Uri")
}

async fn with_timeout<T, F>(f: F, timeout: Option<Duration>) -> Result<T, BoxError>
where
    F: Future<Output = Result<T, BoxError>>,
{
    if let Some(to) = timeout {
        match tokio::time::timeout(to, f).await {
            Err(_elapsed) => Err(Box::new(crate::error::TimedOut) as BoxError),
            Ok(Ok(try_res)) => Ok(try_res),
            Ok(Err(e)) => Err(e),
        }
    } else {
        f.await
    }
}

impl Service<Uri> for Connector {
    type Response = Conn;
    type Error = BoxError;
    type Future = Connecting;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        log::debug!("starting new connection: {:?}", dst);
        let timeout = self.timeout;
        for prox in self.proxies.iter() {
            if let Some(proxy_scheme) = prox.intercept(&dst) {
                return Box::pin(with_timeout(
                    self.clone().connect_via_proxy(dst, proxy_scheme),
                    timeout,
                ));
            }
        }

        Box::pin(with_timeout(
            self.clone().connect_with_maybe_proxy(dst, false),
            timeout,
        ))
    }
}

pub(crate) trait AsyncConn:
    AsyncRead + AsyncWrite + Connection + Send + Sync + Unpin + 'static
{
}

impl<T: AsyncRead + AsyncWrite + Connection + Send + Sync + Unpin + 'static> AsyncConn for T {}

type BoxConn = Box<dyn AsyncConn>;

pin_project! {
    /// Note: the `is_proxy` member means *is plain text HTTP proxy*.
    /// This tells hyper whether the URI should be written in
    /// * origin-form (`GET /just/a/path HTTP/1.1`), when `is_proxy == false`, or
    /// * absolute-form (`GET http://foo.bar/and/a/path HTTP/1.1`), otherwise.
    pub(crate) struct Conn {
        #[pin]
        inner: BoxConn,
        is_proxy: bool,
    }
}

impl Connection for Conn {
    fn connected(&self) -> Connected {
        self.inner.connected().proxy(self.is_proxy)
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();
        AsyncRead::poll_read(this.inner, cx, buf)
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.project();
        AsyncWrite::poll_write(this.inner, cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.project();
        AsyncWrite::poll_write_vectored(this.inner, cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.project();
        AsyncWrite::poll_flush(this.inner, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.project();
        AsyncWrite::poll_shutdown(this.inner, cx)
    }
}

pub(crate) type Connecting = Pin<Box<dyn Future<Output = Result<Conn, BoxError>> + Send>>;

#[cfg(feature = "__tls")]
async fn tunnel<T>(
    mut conn: T,
    host: String,
    port: u16,
    user_agent: Option<HeaderValue>,
    auth: Option<HeaderValue>,
) -> Result<T, BoxError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = format!(
        "\
         CONNECT {0}:{1} HTTP/1.1\r\n\
         Host: {0}:{1}\r\n\
         ",
        host, port
    )
    .into_bytes();

    // user-agent
    if let Some(user_agent) = user_agent {
        buf.extend_from_slice(b"User-Agent: ");
        buf.extend_from_slice(user_agent.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    // proxy-authorization
    if let Some(value) = auth {
        log::debug!("tunnel to {}:{} using basic auth", host, port);
        buf.extend_from_slice(b"Proxy-Authorization: ");
        buf.extend_from_slice(value.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    // headers end
    buf.extend_from_slice(b"\r\n");

    conn.write_all(&buf).await?;

    let mut buf = [0; 8192];
    let mut pos = 0;

    loop {
        let n = conn.read(&mut buf[pos..]).await?;

        if n == 0 {
            return Err(tunnel_eof());
        }
        pos += n;

        let recvd = &buf[..pos];
        if recvd.starts_with(b"HTTP/1.1 200") || recvd.starts_with(b"HTTP/1.0 200") {
            if recvd.ends_with(b"\r\n\r\n") {
                return Ok(conn);
            }
            if pos == buf.len() {
                return Err("proxy headers too long for tunnel".into());
            }
        // else read more
        } else if recvd.starts_with(b"HTTP/1.1 407") {
            return Err("proxy authentication required".into());
        } else {
            return Err("unsuccessful tunnel".into());
        }
    }
}

#[cfg(feature = "__tls")]
fn tunnel_eof() -> BoxError {
    "unexpected eof while tunneling".into()
}

#[cfg(feature = "default-tls")]
mod native_tls_conn {
    use hyper::client::connect::{Connected, Connection};
    use pin_project_lite::pin_project;
    use std::{
        io::{self, IoSlice},
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio_native_tls::TlsStream;

    pin_project! {
        pub(super) struct NativeTlsConn<T> {
            #[pin] pub(super) inner: TlsStream<T>,
        }
    }

    impl<T: Connection + AsyncRead + AsyncWrite + Unpin> Connection for NativeTlsConn<T> {
        #[cfg(feature = "native-tls-alpn")]
        fn connected(&self) -> Connected {
            match self.inner.get_ref().negotiated_alpn().ok() {
                Some(Some(alpn_protocol)) if alpn_protocol == b"h2" => self
                    .inner
                    .get_ref()
                    .get_ref()
                    .get_ref()
                    .connected()
                    .negotiated_h2(),
                _ => self.inner.get_ref().get_ref().get_ref().connected(),
            }
        }

        #[cfg(not(feature = "native-tls-alpn"))]
        fn connected(&self) -> Connected {
            self.inner.get_ref().get_ref().get_ref().connected()
        }
    }

    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for NativeTlsConn<T> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<tokio::io::Result<()>> {
            let this = self.project();
            AsyncRead::poll_read(this.inner, cx, buf)
        }
    }

    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for NativeTlsConn<T> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, tokio::io::Error>> {
            let this = self.project();
            AsyncWrite::poll_write(this.inner, cx, buf)
        }

        fn poll_write_vectored(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<Result<usize, io::Error>> {
            let this = self.project();
            AsyncWrite::poll_write_vectored(this.inner, cx, bufs)
        }

        fn is_write_vectored(&self) -> bool {
            self.inner.is_write_vectored()
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), tokio::io::Error>> {
            let this = self.project();
            AsyncWrite::poll_flush(this.inner, cx)
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), tokio::io::Error>> {
            let this = self.project();
            AsyncWrite::poll_shutdown(this.inner, cx)
        }
    }
}

#[cfg(feature = "__rustls")]
mod rustls_tls_conn {
    use hyper::client::connect::{Connected, Connection};
    use pin_project_lite::pin_project;
    use std::{
        io::{self, IoSlice},
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio_rustls::client::TlsStream;

    pin_project! {
        pub(super) struct RustlsTlsConn<T> {
            #[pin] pub(super) inner: TlsStream<T>,
        }
    }

    impl<T: Connection + AsyncRead + AsyncWrite + Unpin> Connection for RustlsTlsConn<T> {
        fn connected(&self) -> Connected {
            if self.inner.get_ref().1.alpn_protocol() == Some(b"h2") {
                self.inner.get_ref().0.connected().negotiated_h2()
            } else {
                self.inner.get_ref().0.connected()
            }
        }
    }

    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for RustlsTlsConn<T> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<tokio::io::Result<()>> {
            let this = self.project();
            AsyncRead::poll_read(this.inner, cx, buf)
        }
    }

    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for RustlsTlsConn<T> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, tokio::io::Error>> {
            let this = self.project();
            AsyncWrite::poll_write(this.inner, cx, buf)
        }

        fn poll_write_vectored(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<Result<usize, io::Error>> {
            let this = self.project();
            AsyncWrite::poll_write_vectored(this.inner, cx, bufs)
        }

        fn is_write_vectored(&self) -> bool {
            self.inner.is_write_vectored()
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), tokio::io::Error>> {
            let this = self.project();
            AsyncWrite::poll_flush(this.inner, cx)
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), tokio::io::Error>> {
            let this = self.project();
            AsyncWrite::poll_shutdown(this.inner, cx)
        }
    }
}

#[cfg(feature = "socks")]
mod socks {
    use std::io;
    use std::net::ToSocketAddrs;

    use http::Uri;
    use tokio::net::TcpStream;
    use tokio_socks::tcp::Socks5Stream;

    use super::{BoxError, Scheme};
    use crate::proxy::ProxyScheme;

    pub(super) enum DnsResolve {
        Local,
        Proxy,
    }

    pub(super) async fn connect(
        proxy: ProxyScheme,
        dst: Uri,
        dns: DnsResolve,
    ) -> Result<TcpStream, BoxError> {
        let https = dst.scheme() == Some(&Scheme::HTTPS);
        let original_host = dst
            .host()
            .ok_or(io::Error::new(io::ErrorKind::Other, "no host in url"))?;
        let mut host = original_host.to_owned();
        let port = match dst.port() {
            Some(p) => p.as_u16(),
            None if https => 443u16,
            _ => 80u16,
        };

        if let DnsResolve::Local = dns {
            let maybe_new_target = (host.as_str(), port).to_socket_addrs()?.next();
            if let Some(new_target) = maybe_new_target {
                host = new_target.ip().to_string();
            }
        }

        let (socket_addr, auth) = match proxy {
            ProxyScheme::Socks5 { addr, auth, .. } => (addr, auth),
            _ => unreachable!(),
        };

        // Get a Tokio TcpStream
        let stream = if let Some((username, password)) = auth {
            Socks5Stream::connect_with_password(
                socket_addr,
                (host.as_str(), port),
                &username,
                &password,
            )
            .await
            .map_err(|e| format!("socks connect error: {}", e))?
        } else {
            Socks5Stream::connect(socket_addr, (host.as_str(), port))
                .await
                .map_err(|e| format!("socks connect error: {}", e))?
        };

        Ok(stream.into_inner())
    }
}

pub(crate) mod itertools {
    pub(crate) enum Either<A, B> {
        Left(A),
        Right(B),
    }

    impl<A, B> Iterator for Either<A, B>
    where
        A: Iterator,
        B: Iterator<Item = <A as Iterator>::Item>,
    {
        type Item = <A as Iterator>::Item;

        fn next(&mut self) -> Option<Self::Item> {
            match self {
                Either::Left(a) => a.next(),
                Either::Right(b) => b.next(),
            }
        }
    }
}

pin_project! {
    pub(crate) struct WrappedResolverFuture<Fut> {
        #[pin]
        fut: Fut,
    }
}

impl<Fut, FutOutput, FutError> std::future::Future for WrappedResolverFuture<Fut>
where
    Fut: std::future::Future<Output = Result<FutOutput, FutError>>,
    FutOutput: Iterator<Item = SocketAddr>,
{
    type Output = Result<itertools::Either<FutOutput, std::iter::Once<SocketAddr>>, FutError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.fut
            .poll(cx)
            .map(|result| result.map(itertools::Either::Left))
    }
}

#[derive(Clone)]
pub(crate) struct DnsResolverWithOverrides<Resolver>
where
    Resolver: Clone,
{
    dns_resolver: Resolver,
    overrides: Arc<HashMap<String, SocketAddr>>,
}

impl<Resolver: Clone> DnsResolverWithOverrides<Resolver> {
    fn new(dns_resolver: Resolver, overrides: HashMap<String, SocketAddr>) -> Self {
        DnsResolverWithOverrides {
            dns_resolver,
            overrides: Arc::new(overrides),
        }
    }
}

impl<Resolver, Iter> Service<Name> for DnsResolverWithOverrides<Resolver>
where
    Resolver: Service<Name, Response = Iter> + Clone,
    Iter: Iterator<Item = SocketAddr>,
{
    type Response = itertools::Either<Iter, std::iter::Once<SocketAddr>>;
    type Error = <Resolver as Service<Name>>::Error;
    type Future = Either<
        WrappedResolverFuture<<Resolver as Service<Name>>::Future>,
        futures_util::future::Ready<
            Result<itertools::Either<Iter, std::iter::Once<SocketAddr>>, Self::Error>,
        >,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.dns_resolver.poll_ready(cx)
    }

    fn call(&mut self, name: Name) -> Self::Future {
        match self.overrides.get(name.as_str()) {
            Some(dest) => {
                let fut = futures_util::future::ready(Ok(itertools::Either::Right(
                    std::iter::once(dest.to_owned()),
                )));
                Either::Right(fut)
            }
            None => {
                let resolver_fut = self.dns_resolver.call(name);
                let y = WrappedResolverFuture { fut: resolver_fut };
                Either::Left(y)
            }
        }
    }
}

mod verbose {
    use hyper::client::connect::{Connected, Connection};
    use std::fmt;
    use std::io::{self, IoSlice};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    pub(super) const OFF: Wrapper = Wrapper(false);

    #[derive(Clone, Copy)]
    pub(super) struct Wrapper(pub(super) bool);

    impl Wrapper {
        pub(super) fn wrap<T: super::AsyncConn>(&self, conn: T) -> super::BoxConn {
            if self.0 && log::log_enabled!(log::Level::Trace) {
                Box::new(Verbose {
                    // truncate is fine
                    id: crate::util::fast_random() as u32,
                    inner: conn,
                })
            } else {
                Box::new(conn)
            }
        }
    }

    struct Verbose<T> {
        id: u32,
        inner: T,
    }

    impl<T: Connection + AsyncRead + AsyncWrite + Unpin> Connection for Verbose<T> {
        fn connected(&self) -> Connected {
            self.inner.connected()
        }
    }

    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for Verbose<T> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            match Pin::new(&mut self.inner).poll_read(cx, buf) {
                Poll::Ready(Ok(())) => {
                    log::trace!("{:08x} read: {:?}", self.id, Escape(buf.filled()));
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            }
        }
    }

    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for Verbose<T> {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            match Pin::new(&mut self.inner).poll_write(cx, buf) {
                Poll::Ready(Ok(n)) => {
                    log::trace!("{:08x} write: {:?}", self.id, Escape(&buf[..n]));
                    Poll::Ready(Ok(n))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            }
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<Result<usize, io::Error>> {
            Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
        }

        fn is_write_vectored(&self) -> bool {
            self.inner.is_write_vectored()
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    struct Escape<'a>(&'a [u8]);

    impl fmt::Debug for Escape<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "b\"")?;
            for &c in self.0 {
                // https://doc.rust-lang.org/reference.html#byte-escapes
                if c == b'\n' {
                    write!(f, "\\n")?;
                } else if c == b'\r' {
                    write!(f, "\\r")?;
                } else if c == b'\t' {
                    write!(f, "\\t")?;
                } else if c == b'\\' || c == b'"' {
                    write!(f, "\\{}", c as char)?;
                } else if c == b'\0' {
                    write!(f, "\\0")?;
                // ASCII printable
                } else if c >= 0x20 && c < 0x7f {
                    write!(f, "{}", c as char)?;
                } else {
                    write!(f, "\\x{:02x}", c)?;
                }
            }
            write!(f, "\"")?;
            Ok(())
        }
    }
}

#[cfg(feature = "__tls")]
#[cfg(test)]
mod tests {
    use super::tunnel;
    use crate::proxy;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use tokio::net::TcpStream;
    use tokio::runtime;

    static TUNNEL_UA: &str = "tunnel-test/x.y";
    static TUNNEL_OK: &[u8] = b"\
        HTTP/1.1 200 OK\r\n\
        \r\n\
    ";

    macro_rules! mock_tunnel {
        () => {{
            mock_tunnel!(TUNNEL_OK)
        }};
        ($write:expr) => {{
            mock_tunnel!($write, "")
        }};
        ($write:expr, $auth:expr) => {{
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let connect_expected = format!(
                "\
                 CONNECT {0}:{1} HTTP/1.1\r\n\
                 Host: {0}:{1}\r\n\
                 User-Agent: {2}\r\n\
                 {3}\
                 \r\n\
                 ",
                addr.ip(),
                addr.port(),
                TUNNEL_UA,
                $auth
            )
            .into_bytes();

            thread::spawn(move || {
                let (mut sock, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap();
                assert_eq!(&buf[..n], &connect_expected[..]);

                sock.write_all($write).unwrap();
            });
            addr
        }};
    }

    fn ua() -> Option<http::header::HeaderValue> {
        Some(http::header::HeaderValue::from_static(TUNNEL_UA))
    }

    #[test]
    fn test_tunnel() {
        let addr = mock_tunnel!();

        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("new rt");
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, ua(), None).await
        };

        rt.block_on(f).unwrap();
    }

    #[test]
    fn test_tunnel_eof() {
        let addr = mock_tunnel!(b"HTTP/1.1 200 OK");

        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("new rt");
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, ua(), None).await
        };

        rt.block_on(f).unwrap_err();
    }

    #[test]
    fn test_tunnel_non_http_response() {
        let addr = mock_tunnel!(b"foo bar baz hallo");

        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("new rt");
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, ua(), None).await
        };

        rt.block_on(f).unwrap_err();
    }

    #[test]
    fn test_tunnel_proxy_unauthorized() {
        let addr = mock_tunnel!(
            b"\
            HTTP/1.1 407 Proxy Authentication Required\r\n\
            Proxy-Authenticate: Basic realm=\"nope\"\r\n\
            \r\n\
        "
        );

        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("new rt");
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, ua(), None).await
        };

        let error = rt.block_on(f).unwrap_err();
        assert_eq!(error.to_string(), "proxy authentication required");
    }

    #[test]
    fn test_tunnel_basic_auth() {
        let addr = mock_tunnel!(
            TUNNEL_OK,
            "Proxy-Authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==\r\n"
        );

        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("new rt");
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(
                tcp,
                host,
                port,
                ua(),
                Some(proxy::encode_basic_auth("Aladdin", "open sesame")),
            )
            .await
        };

        rt.block_on(f).unwrap();
    }
}
