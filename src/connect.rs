use futures_util::FutureExt;
use http::uri::Scheme;
use hyper::client::connect::{Connect, Connected, Destination};
use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(feature = "default-tls")]
use native_tls::{TlsConnector, TlsConnectorBuilder};

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

//#[cfg(feature = "trust-dns")]
//use crate::dns::TrustDnsResolver;
use crate::proxy::{Proxy, ProxyScheme};
use tokio::future::FutureExt as _;

//#[cfg(feature = "trust-dns")]
//type HttpConnector = hyper::client::HttpConnector<TrustDnsResolver>;
//#[cfg(not(feature = "trust-dns"))]
type HttpConnector = hyper::client::HttpConnector;

pub(crate) struct Connector {
    inner: Inner,
    proxies: Arc<Vec<Proxy>>,
    timeout: Option<Duration>,
    #[cfg(feature = "tls")]
    nodelay: bool,
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
        })
    }

    #[cfg(feature = "rustls-tls")]
    pub(crate) fn new_rustls_tls<T>(
        tls: rustls::ClientConfig,
        proxies: Arc<Vec<Proxy>>,
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

async fn connect_with_maybe_proxy(
    inner: Inner,
    dst: Destination,
    is_proxy: bool,
    no_delay: bool,
) -> Result<(Conn, Connected), io::Error> {
    match inner {
        #[cfg(not(feature = "tls"))]
        Inner::Http(http) => {
            drop(no_delay); // only used for TLS?
            let (io, connected) = http.connect(dst).await?;
            Ok((Box::new(io) as Conn, connected.proxy(is_proxy)))
        }
        #[cfg(feature = "default-tls")]
        Inner::DefaultTls(http, tls) => {
            let mut http = http.clone();

            http.set_nodelay(no_delay || (dst.scheme() == "https"));

            let tls_connector = tokio_tls::TlsConnector::from(tls.clone());
            let http = hyper_tls::HttpsConnector::from((http, tls_connector));
            let (io, connected) = http.connect(dst).await?;
            //TODO: where's this at now?
            //if let hyper_tls::MaybeHttpsStream::Https(_stream) = &io {
            //    if !no_delay {
            //        stream.set_nodelay(false)?;
            //    }
            //}

            Ok((Box::new(io) as Conn, connected.proxy(is_proxy)))
        }
        #[cfg(feature = "rustls-tls")]
        Inner::RustlsTls { http, tls, .. } => {
            let mut http = http.clone();

            // Disable Nagle's algorithm for TLS handshake
            //
            // https://www.openssl.org/docs/man1.1.1/man3/SSL_connect.html#NOTES
            http.set_nodelay(no_delay || (dst.scheme() == "https"));

            let http = hyper_rustls::HttpsConnector::from((http, tls.clone()));
            let (io, connected) = http.connect(dst).await?;
            if let hyper_rustls::MaybeHttpsStream::Https(stream) = &io {
                if !no_delay {
                    let (io, _) = stream.get_ref();
                    io.set_nodelay(false)?;
                }
            }

            Ok((Box::new(io) as Conn, connected.proxy(is_proxy)))
        }
    }
}

async fn connect_via_proxy(
    inner: Inner,
    dst: Destination,
    proxy_scheme: ProxyScheme,
    no_delay: bool,
) -> Result<(Conn, Connected), io::Error> {
    log::trace!("proxy({:?}) intercepts {:?}", proxy_scheme, dst);

    let (puri, _auth) = match proxy_scheme {
        ProxyScheme::Http { uri, auth, .. } => (uri, auth),
        #[cfg(feature = "socks")]
        ProxyScheme::Socks5 { .. } => return this.connect_socks(dst, proxy_scheme),
    };

    let mut ndst = dst.clone();

    let new_scheme = puri.scheme_part().map(Scheme::as_str).unwrap_or("http");
    ndst.set_scheme(new_scheme)
        .expect("proxy target scheme should be valid");

    ndst.set_host(puri.host().expect("proxy target should have host"))
        .expect("proxy target host should be valid");

    ndst.set_port(puri.port_part().map(|port| port.as_u16()));

    #[cfg(feature = "tls")]
    let auth = _auth;

    match &inner {
        #[cfg(feature = "default-tls")]
        Inner::DefaultTls(http, tls) => {
            if dst.scheme() == "https" {
                let host = dst.host().to_owned();
                let port = dst.port().unwrap_or(443);
                let mut http = http.clone();
                http.set_nodelay(no_delay);
                let tls_connector = tokio_tls::TlsConnector::from(tls.clone());
                let http = hyper_tls::HttpsConnector::from((http, tls_connector));
                let (conn, connected) = http.connect(ndst).await?;
                log::trace!("tunneling HTTPS over proxy");
                let tunneled = tunnel(conn, host.clone(), port, auth).await?;
                let tls_connector = tokio_tls::TlsConnector::from(tls.clone());
                let io = tls_connector
                    .connect(&host, tunneled)
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                return Ok((Box::new(io) as Conn, connected.proxy(true)));
            }
        }
        #[cfg(feature = "rustls-tls")]
        Inner::RustlsTls {
            http,
            tls,
            tls_proxy,
        } => {
            if dst.scheme() == "https" {
                use rustls::Session;
                use tokio_rustls::webpki::DNSNameRef;
                use tokio_rustls::TlsConnector as RustlsConnector;

                let host = dst.host().to_owned();
                let port = dst.port().unwrap_or(443);
                let mut http = http.clone();
                http.set_nodelay(no_delay);
                let http = hyper_rustls::HttpsConnector::from((http, tls_proxy.clone()));
                let tls = tls.clone();
                let (conn, connected) = http.connect(ndst).await?;
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
                return Ok((Box::new(io) as Conn, connected.proxy(true)));
            }
        }
        #[cfg(not(feature = "tls"))]
        Inner::Http(_) => (),
    }

    connect_with_maybe_proxy(inner, ndst, true, no_delay).await
}

async fn with_timeout<T, F>(f: F, timeout: Option<Duration>) -> Result<T, io::Error>
where
    F: Future<Output = Result<T, io::Error>>,
{
    if let Some(to) = timeout {
        match f.timeout(to).await {
            Err(_elapsed) => Err(io::Error::new(io::ErrorKind::TimedOut, "connect timed out")),
            Ok(try_res) => try_res,
        }
    } else {
        f.await
    }
}

impl Connect for Connector {
    type Transport = Conn;
    type Error = io::Error;
    type Future = Connecting;

    fn connect(&self, dst: Destination) -> Self::Future {
        #[cfg(feature = "tls")]
        let no_delay = self.nodelay;
        #[cfg(not(feature = "tls"))]
        let no_delay = false;
        let timeout = self.timeout;
        for prox in self.proxies.iter() {
            if let Some(proxy_scheme) = prox.intercept(&dst) {
                return with_timeout(
                    connect_via_proxy(self.inner.clone(), dst, proxy_scheme, no_delay),
                    timeout,
                )
                .boxed();
            }
        }

        with_timeout(
            connect_with_maybe_proxy(self.inner.clone(), dst, false, no_delay),
            timeout,
        )
        .boxed()
    }
}

pub(crate) trait AsyncConn: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncConn for T {}
pub(crate) type Conn = Box<dyn AsyncConn + Send + Sync + Unpin + 'static>;

pub(crate) type Connecting =
    Pin<Box<dyn Future<Output = Result<(Conn, Connected), io::Error>> + Send>>;

#[cfg(feature = "tls")]
async fn tunnel<T>(
    mut conn: T,
    host: String,
    port: u16,
    auth: Option<http::header::HeaderValue>,
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
    ) -> Result<(super::Conn, Connected), io::Error> {
        let https = dst.scheme() == "https";
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
    use tokio::net::tcp::TcpStream;
    use tokio::runtime::current_thread::Runtime;

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
                 {2}\
                 \r\n\
                 ",
                addr.ip(),
                addr.port(),
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

    #[test]
    fn test_tunnel() {
        let addr = mock_tunnel!();

        let mut rt = Runtime::new().unwrap();
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, None).await
        };

        rt.block_on(f).unwrap();
    }

    #[test]
    fn test_tunnel_eof() {
        let addr = mock_tunnel!(b"HTTP/1.1 200 OK");

        let mut rt = Runtime::new().unwrap();
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, None).await
        };

        rt.block_on(f).unwrap_err();
    }

    #[test]
    fn test_tunnel_non_http_response() {
        let addr = mock_tunnel!(b"foo bar baz hallo");

        let mut rt = Runtime::new().unwrap();
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, None).await
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

        let mut rt = Runtime::new().unwrap();
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(tcp, host, port, None).await
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

        let mut rt = Runtime::new().unwrap();
        let f = async move {
            let tcp = TcpStream::connect(&addr).await?;
            let host = addr.ip().to_string();
            let port = addr.port();
            tunnel(
                tcp,
                host,
                port,
                Some(proxy::encode_basic_auth("Aladdin", "open sesame")),
            )
            .await
        };

        rt.block_on(f).unwrap();
    }
}
