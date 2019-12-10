use futures_util::FutureExt;
use hyper::service::Service;
use http::uri::{Scheme, Authority};
use http::Uri;
use hyper::client::connect::{Connected, Connection};
use tokio::io::{AsyncRead, AsyncWrite};
#[cfg(feature = "default-tls")]
use native_tls::{TlsConnector, TlsConnectorBuilder};
#[cfg(feature = "tls")]
use http::header::HeaderValue;
use bytes::{Buf, BufMut};

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use std::mem::MaybeUninit;
use pin_project_lite::pin_project;

//#[cfg(feature = "trust-dns")]
//use crate::dns::TrustDnsResolver;
use crate::proxy::{Proxy, ProxyScheme};
use crate::error::BoxError;

//#[cfg(feature = "trust-dns")]
//type HttpConnector = hyper::client::HttpConnector<TrustDnsResolver>;
//#[cfg(not(feature = "trust-dns"))]
type HttpConnector = hyper::client::HttpConnector;

#[derive(Clone)]
pub(crate) struct Connector {
    inner: Inner,
    proxies: Arc<Vec<Proxy>>,
    timeout: Option<Duration>,
    #[cfg(feature = "tls")]
    nodelay: bool,
    #[cfg(feature = "tls")]
    user_agent: HeaderValue,
}

#[derive(Clone)]
enum Inner {
    #[cfg(not(feature = "tls"))]
    Http(HttpConnector),
    #[cfg(feature = "default-tls")]
    DefaultTls(HttpConnector, TlsConnector),
    #[cfg(feature = "rustls-tls")]
    RustlsTls {
        http: HttpConnector,
        tls: Arc<rustls::ClientConfig>,
        tls_proxy: Arc<rustls::ClientConfig>,
    },
}

impl Connector {
    #[cfg(not(feature = "tls"))]
    pub(crate) fn new<T>(
        proxies: Arc<Vec<Proxy>>,
        local_addr: T,
        nodelay: bool,
    ) -> crate::Result<Connector>
    where
        T: Into<Option<IpAddr>>,
    {
        let mut http = http_connector()?;
        http.set_local_address(local_addr.into());
        http.set_nodelay(nodelay);
        Ok(Connector {
            inner: Inner::Http(http),
            proxies,
            timeout: None,
        })
    }

    #[cfg(feature = "default-tls")]
    pub(crate) fn new_default_tls<T>(
        tls: TlsConnectorBuilder,
        proxies: Arc<Vec<Proxy>>,
        user_agent: HeaderValue,
        local_addr: T,
        nodelay: bool,
    ) -> crate::Result<Connector>
    where
        T: Into<Option<IpAddr>>,
    {
        let tls = tls.build().map_err(crate::error::builder)?;

        let mut http = http_connector()?;
        http.set_local_address(local_addr.into());
        http.enforce_http(false);

        Ok(Connector {
            inner: Inner::DefaultTls(http, tls),
            proxies,
            timeout: None,
            nodelay,
            user_agent,
        })
    }

    #[cfg(feature = "rustls-tls")]
    pub(crate) fn new_rustls_tls<T>(
        tls: rustls::ClientConfig,
        proxies: Arc<Vec<Proxy>>,
        user_agent: HeaderValue,
        local_addr: T,
        nodelay: bool,
    ) -> crate::Result<Connector>
    where
        T: Into<Option<IpAddr>>,
    {
        let mut http = http_connector()?;
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

        Ok(Connector {
            inner: Inner::RustlsTls {
                http,
                tls,
                tls_proxy,
            },
            proxies,
            timeout: None,
            nodelay,
            user_agent,
        })
    }

    pub(crate) fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    #[cfg(feature = "socks")]
    async fn connect_socks(
        &self,
        dst: Destination,
        proxy: ProxyScheme,
    ) -> Result<(Conn, Connected), io::Error> {
        let dns = match proxy {
            ProxyScheme::Socks5 {
                remote_dns: false, ..
            } => socks::DnsResolve::Local,
            ProxyScheme::Socks5 {
                remote_dns: true, ..
            } => socks::DnsResolve::Proxy,
            ProxyScheme::Http { .. } => {
                unreachable!("connect_socks is only called for socks proxies");
            }
        };

        match &self.inner {
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(_http, tls) => {
                if dst.scheme() == "https" {
                    use self::native_tls_async::TlsConnectorExt;

                    let host = dst.host().to_owned();
                    let socks_connecting = socks::connect(proxy, dst, dns);
                    let (conn, connected) = socks::connect(proxy, dst, dns).await?;
                    let tls_connector = tokio_tls::TlsConnector::from(tls.clone());
                    let io = tls_connector
                        .connect(&host, conn)
                        .await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    Ok((Box::new(io) as Conn, connected))
                }
            }
            #[cfg(feature = "rustls-tls")]
            Inner::RustlsTls { tls_proxy, .. } => {
                if dst.scheme() == "https" {
                    use tokio_rustls::webpki::DNSNameRef;
                    use tokio_rustls::TlsConnector as RustlsConnector;

                    let tls = tls_proxy.clone();
                    let host = dst.host().to_owned();
                    let (conn, connected) = socks::connect(proxy, dst, dns);
                    let dnsname = DNSNameRef::try_from_ascii_str(&host)
                        .map(|dnsname| dnsname.to_owned())
                        .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid DNS Name"))?;
                    let io = RustlsConnector::from(tls)
                        .connect(dnsname.as_ref(), conn)
                        .await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    Ok((Box::new(io) as Conn, connected))
                }
            }
            #[cfg(not(feature = "tls"))]
            Inner::Http(_) => socks::connect(proxy, dst, dns),
        }
    }

    async fn connect_with_maybe_proxy(
        self,
        dst: Uri,
        is_proxy: bool,
    ) -> Result<Conn, BoxError> {
        match self.inner {
            #[cfg(not(feature = "tls"))]
            Inner::Http(http) => {
                let io = http.connect(dst).await?;
                Ok(Conn {
                    inner: Box::new(io),
                    is_proxy,
                })
            }
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(http, tls) => {
                let mut http = http.clone();

                http.set_nodelay(self.nodelay || (dst.scheme() == Some(&Scheme::HTTPS)));

                let tls_connector = tokio_tls::TlsConnector::from(tls.clone());
                let mut http = hyper_tls::HttpsConnector::from((http, tls_connector));
                let io = http.call(dst).await?;
                //TODO: where's this at now?
                //if let hyper_tls::MaybeHttpsStream::Https(_stream) = &io {
                //    if !no_delay {
                //        stream.set_nodelay(false)?;
                //    }
                //}

                Ok(Conn {
                    inner: Box::new(io),
                    is_proxy,
                })
            }
            #[cfg(feature = "rustls-tls")]
            Inner::RustlsTls { http, tls, .. } => {
                let mut http = http.clone();

                // Disable Nagle's algorithm for TLS handshake
                //
                // https://www.openssl.org/docs/man1.1.1/man3/SSL_connect.html#NOTES
                http.set_nodelay(no_delay || (dst.scheme() == Some(&Scheme::HTTPS)));

                let http = hyper_rustls::HttpsConnector::from((http, tls.clone()));
                let io = http.connect(dst).await?;
                if let hyper_rustls::MaybeHttpsStream::Https(stream) = &io {
                    if !no_delay {
                        let (io, _) = stream.get_ref();
                        io.set_nodelay(false)?;
                    }
                }

                Ok(Conn {
                    inner: Box::new(io),
                    is_proxy,
                })
            }
        }
    }

    async fn connect_via_proxy(
        self,
        dst: Uri,
        proxy_scheme: ProxyScheme,
    ) -> Result<Conn, BoxError> {
        log::trace!("proxy({:?}) intercepts {:?}", proxy_scheme, dst);

        let (proxy_dst, _auth) = match proxy_scheme {
            ProxyScheme::Http { host, auth } => (into_uri(Scheme::HTTP, host), auth),
            ProxyScheme::Https { host, auth } => (into_uri(Scheme::HTTPS, host), auth),
            #[cfg(feature = "socks")]
            ProxyScheme::Socks5 { .. } => return this.connect_socks(dst, proxy_scheme),
        };


        #[cfg(feature = "tls")]
        let auth = _auth;

        match &self.inner {
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(http, tls) => {
                if dst.scheme() == Some(&Scheme::HTTPS) {
                    let host = dst.host().to_owned();
                    let port = dst.port().map(|p| p.as_u16()).unwrap_or(443);
                    let mut http = http.clone();
                    http.set_nodelay(self.nodelay);
                    let tls_connector = tokio_tls::TlsConnector::from(tls.clone());
                    let mut http = hyper_tls::HttpsConnector::from((http, tls_connector));
                    let conn = http.call(proxy_dst).await?;
                    log::trace!("tunneling HTTPS over proxy");
                    let tunneled = tunnel(
                        conn,
                        host
                            .ok_or(io::Error::new(io::ErrorKind::Other, "no host in url"))?
                            .to_string(),
                        port,
                        self.user_agent.clone(),
                        auth
                    ).await?;
                    let tls_connector = tokio_tls::TlsConnector::from(tls.clone());
                    let io = tls_connector
                        .connect(&host.ok_or(io::Error::new(io::ErrorKind::Other, "no host in url"))?, tunneled)
                        .await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    return Ok(Conn {
                        inner: Box::new(io),
                        is_proxy: false,
                    });
                }
            }
            #[cfg(feature = "rustls-tls")]
            Inner::RustlsTls {
                http,
                tls,
                tls_proxy,
            } => {
                if dst.scheme() == Some(&Scheme::HTTPS) {
                    use rustls::Session;
                    use tokio_rustls::webpki::DNSNameRef;
                    use tokio_rustls::TlsConnector as RustlsConnector;

                    let host = dst.host().to_owned();
                    let port = dst.port().unwrap_or(443);
                    let mut http = http.clone();
                    http.set_nodelay(no_delay);
                    let http = hyper_rustls::HttpsConnector::from((http, tls_proxy.clone()));
                    let tls = tls.clone();
                    let (conn, connected) = http.connect(proxy_dst).await?;
                    log::trace!("tunneling HTTPS over proxy");
                    let maybe_dnsname = DNSNameRef::try_from_ascii_str(&host)
                        .map(|dnsname| dnsname.to_owned())
                        .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid DNS Name"));
                    let tunneled = tunnel(conn, host, port, auth).await?;
                    let dnsname = maybe_dnsname?;
                    let io = RustlsConnector::from(tls)
                        .connect(dnsname.as_ref(), tunneled)
                        .await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    let connected = if io.get_ref().1.get_alpn_protocol() == Some(b"h2") {
                        connected.negotiated_h2()
                    } else {
                        connected
                    };
                    return Ok(Conn {
                        inner: Box::new(io),
                        connected: Connected::new(),
                    });
                }
            }
            #[cfg(not(feature = "tls"))]
            Inner::Http(_) => (),
        }

        self.connect_with_maybe_proxy(proxy_dst, true).await
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

//#[cfg(feature = "trust-dns")]
//fn http_connector() -> crate::Result<HttpConnector> {
//    TrustDnsResolver::new()
//        .map(HttpConnector::new_with_resolver)
//        .map_err(crate::error::dns_system_conf)
//}

//#[cfg(not(feature = "trust-dns"))]
fn http_connector() -> crate::Result<HttpConnector> {
    Ok(HttpConnector::new())
}


async fn with_timeout<T, F>(f: F, timeout: Option<Duration>) -> Result<T, BoxError>
where
    F: Future<Output = Result<T, BoxError>>,
{
    if let Some(to) = timeout {
        match tokio::time::timeout(to, f).await {
            Err(_elapsed) => Err(Box::new(io::Error::new(io::ErrorKind::TimedOut, "connect timed out")) as BoxError),
            Ok(Ok(try_res)) => Ok(try_res),
            Ok(Err(e)) => Err(e),
        }
    } else {
        f.await
    }
}

impl Service<Uri> for Connector
{
    type Response = Conn;
    type Error = BoxError;
    type Future = Connecting;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let timeout = self.timeout;
        for prox in self.proxies.iter() {
            if let Some(proxy_scheme) = prox.intercept(&dst) {
                return with_timeout(
                    self.clone().connect_via_proxy(dst, proxy_scheme),
                    timeout,
                )
                .boxed();
            }
        }

        with_timeout(
            self.clone().connect_with_maybe_proxy(dst, false),
            timeout,
        )
        .boxed()
    }
}

//impl Connect for Connector {
//    type Transport = Conn;
//    type Error = BoxError;
//    type Future = Connecting;
//
//    fn connect(&self, dst: Uri) -> Self::Future {
//        let timeout = self.timeout;
//        for prox in self.proxies.iter() {
//            if let Some(proxy_scheme) = prox.intercept(&dst) {
//                return with_timeout(
//                    self.clone().connect_via_proxy(dst, proxy_scheme),
//                    timeout,
//                )
//                    .boxed();
//            }
//        }
//
//        with_timeout(
//            self.clone().connect_with_maybe_proxy(dst, false),
//            timeout,
//        )
//            .boxed()
//    }
//}


pub(crate) trait AsyncConn: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncConn for T {}

pin_project! {
    pub(crate) struct Conn {
        #[pin]
        inner: Box<dyn AsyncConn + Send + Sync + Unpin + 'static>,
        is_proxy: bool,
    }
}

impl Connection for Conn {
    fn connected(&self) -> Connected {
        Connected::new().proxy(self.is_proxy)
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8]
    ) -> Poll<tokio::io::Result<usize>> {
        let this = self.project();
        AsyncRead::poll_read(this.inner, cx, buf)
    }

    unsafe fn prepare_uninitialized_buffer(
        &self,
        buf: &mut [MaybeUninit<u8>]
    ) -> bool {
        self.inner.prepare_uninitialized_buffer(buf)
    }

    fn poll_read_buf<B: BufMut>(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut B
    ) -> Poll<tokio::io::Result<usize>>
        where
            Self: Sized
    {
        let this = self.project();
        AsyncRead::poll_read_buf(this.inner, cx, buf)
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8]
    ) -> Poll<Result<usize, tokio::io::Error>> {
        let this = self.project();
        AsyncWrite::poll_write(this.inner, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), tokio::io::Error>> {
        let this = self.project();
        AsyncWrite::poll_flush(this.inner, cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context
    ) -> Poll<Result<(), tokio::io::Error>> {
        let this = self.project();
        AsyncWrite::poll_shutdown(this.inner, cx)
    }

    fn poll_write_buf<B: Buf>(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut B
    ) -> Poll<Result<usize, tokio::io::Error>> where
        Self: Sized {
        let this = self.project();
        AsyncWrite::poll_write_buf(this.inner, cx, buf)
    }
}

pub(crate) type Connecting =
    Pin<Box<dyn Future<Output = Result<Conn, BoxError>> + Send>>;

#[cfg(feature = "tls")]
async fn tunnel<T>(
    mut conn: T,
    host: String,
    port: u16,
    user_agent: HeaderValue,
    auth: Option<HeaderValue>,
) -> Result<T, io::Error>
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
    buf.extend_from_slice(b"User-Agent: ");
    buf.extend_from_slice(user_agent.as_bytes());
    buf.extend_from_slice(b"\r\n");


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
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "proxy headers too long for tunnel",
                ));
            }
        // else read more
        } else if recvd.starts_with(b"HTTP/1.1 407") {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "proxy authentication required",
            ));
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, "unsuccessful tunnel"));
        }
    }
}

#[cfg(feature = "tls")]
fn tunnel_eof() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "unexpected eof while tunneling",
    )
}

#[cfg(feature = "socks")]
mod socks {
    use std::io;

    use futures::{future, Future};
    use hyper::client::connect::{Connected, Destination};
    use socks::Socks5Stream;
    use std::net::ToSocketAddrs;
    use tokio::{net::TcpStream, reactor};

    use super::Connecting;
    use crate::proxy::ProxyScheme;

    pub(super) enum DnsResolve {
        Local,
        Proxy,
    }

    pub(super) async fn connect(
        proxy: ProxyScheme,
        dst: Destination,
        dns: DnsResolve,
    ) -> Result<super::Conn, BoxError> {
        let https = dst.scheme() == Some(&Scheme::HTTPS);
        let original_host = dst.host().to_owned();
        let mut host = original_host.clone();
        let port = dst.port().unwrap_or_else(|| if https { 443 } else { 80 });

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
        } else {
            let s = Socks5Stream::connect(socket_addr, (host.as_str(), port)).await;
            TcpStream::from_std(s.into_inner(), &reactor::Handle::default())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };

        Ok((Box::new(s) as super::Conn, Connected::new()))
    }
}

#[cfg(feature = "tls")]
#[cfg(test)]
mod tests {
    use super::tunnel;
    use crate::proxy;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use tokio::net::TcpStream;
    use tokio::runtime;

    static TUNNEL_UA: &'static str = "tunnel-test/x.y";
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

    fn ua() -> http::header::HeaderValue {
        http::header::HeaderValue::from_static(TUNNEL_UA)
    }

    #[test]
    fn test_tunnel() {
        let addr = mock_tunnel!();

        let mut rt = runtime::Builder::new().basic_scheduler().enable_all().build().expect("new rt");
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

        let mut rt = runtime::Builder::new().basic_scheduler().enable_all().build().expect("new rt");
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

        let mut rt = runtime::Builder::new().basic_scheduler().enable_all().build().expect("new rt");
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

        let mut rt = runtime::Builder::new().basic_scheduler().enable_all().build().expect("new rt");
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

        let mut rt = runtime::Builder::new().basic_scheduler().enable_all().build().expect("new rt");
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
