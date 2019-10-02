use futures::Future;
use http::uri::Scheme;
use hyper::client::connect::{Connect, Connected, Destination};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Timeout;


#[cfg(feature = "default-tls")]
use native_tls::{TlsConnector, TlsConnectorBuilder};
#[cfg(feature = "tls")]
use futures::Poll;
#[cfg(feature = "tls")]
use bytes::BufMut;
#[cfg(feature = "tls")]
use http::header::HeaderValue;

use std::io;
use std::sync::Arc;
use std::net::IpAddr;
use std::time::Duration;

#[cfg(feature = "trust-dns")]
use dns::TrustDnsResolver;
use proxy::{Proxy, ProxyScheme};

#[cfg(feature = "trust-dns")]
type HttpConnector = ::hyper::client::HttpConnector<TrustDnsResolver>;
#[cfg(not(feature = "trust-dns"))]
type HttpConnector = ::hyper::client::HttpConnector;


pub(crate) struct Connector {
    inner: Inner,
    proxies: Arc<Vec<Proxy>>,
    timeout: Option<Duration>,
    #[cfg(feature = "tls")]
    nodelay: bool,
    #[cfg(feature = "tls")]
    user_agent: HeaderValue,
}

enum Inner {
    #[cfg(not(feature = "tls"))]
    Http(HttpConnector),
    #[cfg(feature = "default-tls")]
    DefaultTls(HttpConnector, TlsConnector),
    #[cfg(feature = "rustls-tls")]
    RustlsTls {
        http: HttpConnector,
        tls: Arc<rustls::ClientConfig>,
        tls_proxy: Arc<rustls::ClientConfig>
    }
}

impl Connector {
    #[cfg(not(feature = "tls"))]
    pub(crate) fn new<T>(proxies: Arc<Vec<Proxy>>, local_addr: T, nodelay: bool) -> ::Result<Connector>
    where
        T: Into<Option<IpAddr>>
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
        nodelay: bool) -> ::Result<Connector>
        where
            T: Into<Option<IpAddr>>,
    {
        let tls = try_!(tls.build());

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
        nodelay: bool) -> ::Result<Connector>
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
            inner: Inner::RustlsTls { http, tls, tls_proxy },
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
    fn connect_socks(&self, dst: Destination, proxy: ProxyScheme) -> Connecting {
        macro_rules! timeout {
            ($future:expr) => {
                if let Some(dur) = self.timeout {
                    Box::new(Timeout::new($future, dur).map_err(|err| {
                        if err.is_inner() {
                            err.into_inner().expect("is_inner")
                        } else if err.is_elapsed() {
                            io::Error::new(io::ErrorKind::TimedOut, "connect timed out")
                        } else {
                            io::Error::new(io::ErrorKind::Other, err)
                        }
                    }))
                } else {
                    Box::new($future)
                }
            }
        }

        let dns = match proxy {
            ProxyScheme::Socks5 { remote_dns: false, .. } => socks::DnsResolve::Local,
            ProxyScheme::Socks5 { remote_dns: true, .. } => socks::DnsResolve::Proxy,
            ProxyScheme::Http { .. } => {
                unreachable!("connect_socks is only called for socks proxies");
            },
        };


        match &self.inner {
            #[cfg(feature = "default-tls")]
            Inner::DefaultTls(_http, tls) => if dst.scheme() == "https" {
                use self::native_tls_async::TlsConnectorExt;

                let tls = tls.clone();
                let host = dst.host().to_owned();
                let socks_connecting = socks::connect(proxy, dst, dns);
                return timeout!(socks_connecting.and_then(move |(conn, connected)| {
                    tls.connect_async(&host, conn)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                        .map(move |io| (Box::new(io) as Conn, connected))
                }));
            },
            #[cfg(feature = "rustls-tls")]
            Inner::RustlsTls { tls_proxy, .. } => if dst.scheme() == "https" {
                use tokio_rustls::TlsConnector as RustlsConnector;
                use tokio_rustls::webpki::DNSNameRef;

                let tls = tls_proxy.clone();
                let host = dst.host().to_owned();
                let socks_connecting = socks::connect(proxy, dst, dns);
                return timeout!(socks_connecting.and_then(move |(conn, connected)| {
                    let maybe_dnsname = DNSNameRef::try_from_ascii_str(&host)
                        .map(|dnsname| dnsname.to_owned())
                        .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid DNS Name"));
                    futures::future::result(maybe_dnsname)
                        .and_then(move |dnsname| {
                            RustlsConnector::from(tls).connect(dnsname.as_ref(), conn)
                                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                        })
                        .map(move |io| {
                            (Box::new(io) as Conn, connected)
                        })
                }));
            },
            #[cfg(not(feature = "tls"))]
            Inner::Http(_) => ()
        }

        // else no TLS
        socks::connect(proxy, dst, dns)
    }
}

#[cfg(feature = "trust-dns")]
fn http_connector() -> ::Result<HttpConnector> {
    TrustDnsResolver::new()
        .map(HttpConnector::new_with_resolver)
        .map_err(::error::dns_system_conf)
}

#[cfg(not(feature = "trust-dns"))]
fn http_connector() -> ::Result<HttpConnector> {
    Ok(HttpConnector::new(4))
}

impl Connect for Connector {
    type Transport = Conn;
    type Error = io::Error;
    type Future = Connecting;

    fn connect(&self, dst: Destination) -> Self::Future {
        #[cfg(feature = "tls")]
        let nodelay = self.nodelay;

        macro_rules! timeout {
            ($future:expr) => {
                if let Some(dur) = self.timeout {
                    Box::new(Timeout::new($future, dur).map_err(|err| {
                        if err.is_inner() {
                            err.into_inner().expect("is_inner")
                        } else if err.is_elapsed() {
                            io::Error::new(io::ErrorKind::TimedOut, "connect timed out")
                        } else {
                            io::Error::new(io::ErrorKind::Other, err)
                        }
                    }))
                } else {
                    Box::new($future)
                }
            }
        }

        macro_rules! connect {
            ( $http:expr, $dst:expr, $proxy:expr ) => {
                timeout!($http.connect($dst)
                    .map(|(io, connected)| (Box::new(io) as Conn, connected.proxy($proxy))))
            };
            ( $dst:expr, $proxy:expr ) => {
                match &self.inner {
                    #[cfg(not(feature = "tls"))]
                    Inner::Http(http) => connect!(http, $dst, $proxy),
                    #[cfg(feature = "default-tls")]
                    Inner::DefaultTls(http, tls) => {
                        let mut http = http.clone();

                        http.set_nodelay(nodelay || ($dst.scheme() == "https"));

                        let http = ::hyper_tls::HttpsConnector::from((http, tls.clone()));
                        timeout!(http.connect($dst)
                            .and_then(move |(io, connected)| {
                                if let ::hyper_tls::MaybeHttpsStream::Https(stream) = &io {
                                    if !nodelay {
                                        stream.get_ref().get_ref().set_nodelay(false)?;
                                    }
                                }

                                Ok((Box::new(io) as Conn, connected.proxy($proxy)))
                            }))
                    },
                    #[cfg(feature = "rustls-tls")]
                    Inner::RustlsTls { http, tls, .. } => {
                        let mut http = http.clone();

                        // Disable Nagle's algorithm for TLS handshake
                        //
                        // https://www.openssl.org/docs/man1.1.1/man3/SSL_connect.html#NOTES
                        http.set_nodelay(nodelay || ($dst.scheme() == "https"));

                        let http = ::hyper_rustls::HttpsConnector::from((http, tls.clone()));
                        timeout!(http.connect($dst)
                            .and_then(move |(io, connected)| {
                                if let ::hyper_rustls::MaybeHttpsStream::Https(stream) = &io {
                                    if !nodelay {
                                        let (io, _) = stream.get_ref();
                                        io.set_nodelay(false)?;
                                    }
                                }

                                Ok((Box::new(io) as Conn, connected.proxy($proxy)))
                            }))
                    }
                }
            };
        }

        for prox in self.proxies.iter() {
            if let Some(proxy_scheme) = prox.intercept(&dst) {
                trace!("proxy({:?}) intercepts {:?}", proxy_scheme, dst);

                let (puri, _auth) = match proxy_scheme {
                    ProxyScheme::Http { uri, auth, .. } => (uri, auth),
                    #[cfg(feature = "socks")]
                    ProxyScheme::Socks5 { .. } => return self.connect_socks(dst, proxy_scheme),
                };

                let mut ndst = dst.clone();

                let new_scheme = puri
                    .scheme_part()
                    .map(Scheme::as_str)
                    .unwrap_or("http");
                ndst.set_scheme(new_scheme)
                    .expect("proxy target scheme should be valid");

                ndst.set_host(puri.host().expect("proxy target should have host"))
                    .expect("proxy target host should be valid");

                ndst.set_port(puri.port_part().map(|port| port.as_u16()));

                #[cfg(feature = "tls")]
                let auth = _auth;

                match &self.inner {
                    #[cfg(feature = "default-tls")]
                    Inner::DefaultTls(http, tls) => if dst.scheme() == "https" {
                        use self::native_tls_async::TlsConnectorExt;

                        let host = dst.host().to_owned();
                        let port = dst.port().unwrap_or(443);
                        let mut http = http.clone();
                        http.set_nodelay(nodelay);
                        let http = ::hyper_tls::HttpsConnector::from((http, tls.clone()));
                        let tls = tls.clone();
                        let user_agent = self.user_agent.clone();
                        return timeout!(http.connect(ndst).and_then(move |(conn, connected)| {
                            trace!("tunneling HTTPS over proxy");
                            tunnel(conn, host.clone(), port, user_agent, auth)
                                .and_then(move |tunneled| {
                                    tls.connect_async(&host, tunneled)
                                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                                })
                                .map(|io| (Box::new(io) as Conn, connected.proxy(true)))
                        }));
                    },
                    #[cfg(feature = "rustls-tls")]
                    Inner::RustlsTls { http, tls, tls_proxy } => if dst.scheme() == "https" {
                        use rustls::Session;
                        use tokio_rustls::TlsConnector as RustlsConnector;
                        use tokio_rustls::webpki::DNSNameRef;

                        let host = dst.host().to_owned();
                        let port = dst.port().unwrap_or(443);
                        let mut http = http.clone();
                        http.set_nodelay(nodelay);
                        let http = ::hyper_rustls::HttpsConnector::from((http, tls_proxy.clone()));
                        let tls = tls.clone();
                        let user_agent = self.user_agent.clone();
                        return timeout!(http.connect(ndst).and_then(move |(conn, connected)| {
                            trace!("tunneling HTTPS over proxy");
                            let maybe_dnsname = DNSNameRef::try_from_ascii_str(&host)
                                .map(|dnsname| dnsname.to_owned())
                                .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid DNS Name"));
                            tunnel(conn, host, port, user_agent, auth)
                                .and_then(move |tunneled| Ok((maybe_dnsname?, tunneled)))
                                .and_then(move |(dnsname, tunneled)| {
                                    RustlsConnector::from(tls).connect(dnsname.as_ref(), tunneled)
                                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                                })
                                .map(|io| {
                                    let connected = if io.get_ref().1.get_alpn_protocol() == Some(b"h2") {
                                        connected.negotiated_h2()
                                    } else {
                                        connected
                                    };
                                    (Box::new(io) as Conn, connected.proxy(true))
                                })
                        }));
                    },
                    #[cfg(not(feature = "tls"))]
                    Inner::Http(_) => ()
                }

                return connect!(ndst, true);
            }
        }

        connect!(dst, false)
    }
}

pub(crate) trait AsyncConn: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncConn for T {}
pub(crate) type Conn = Box<dyn AsyncConn + Send + Sync + 'static>;

pub(crate) type Connecting = Box<dyn Future<Item=(Conn, Connected), Error=io::Error> + Send>;

#[cfg(feature = "tls")]
fn tunnel<T>(conn: T, host: String, port: u16, user_agent: HeaderValue, auth: Option<HeaderValue>) -> Tunnel<T> {
    let mut buf = format!("\
        CONNECT {0}:{1} HTTP/1.1\r\n\
        Host: {0}:{1}\r\n\
    ", host, port).into_bytes();

    // user-agent
    buf.extend_from_slice(b"User-Agent: ");
    buf.extend_from_slice(user_agent.as_bytes());
    buf.extend_from_slice(b"\r\n");

    // proxy-authorization
    match auth {
        Some(value) => {
            debug!("tunnel to {}:{} using basic auth", host, port);
            buf.extend_from_slice(b"Proxy-Authorization: ");
            buf.extend_from_slice(value.as_bytes());
            buf.extend_from_slice(b"\r\n");
        },
        None => (),
    }

    // headers end
    buf.extend_from_slice(b"\r\n");

    Tunnel {
        buf: io::Cursor::new(buf),
        conn: Some(conn),
        state: TunnelState::Writing,
    }
}

#[cfg(feature = "tls")]
struct Tunnel<T> {
    buf: io::Cursor<Vec<u8>>,
    conn: Option<T>,
    state: TunnelState,
}

#[cfg(feature = "tls")]
enum TunnelState {
    Writing,
    Reading
}

#[cfg(feature = "tls")]
impl<T> Future for Tunnel<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Item = T;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            if let TunnelState::Writing = self.state {
                let n = try_ready!(self.conn.as_mut().unwrap().write_buf(&mut self.buf));
                if !self.buf.has_remaining_mut() {
                    self.state = TunnelState::Reading;
                    self.buf.get_mut().truncate(0);
                } else if n == 0 {
                    return Err(tunnel_eof());
                }
            } else {
                let n = try_ready!(self.conn.as_mut().unwrap().read_buf(&mut self.buf.get_mut()));
                let read = &self.buf.get_ref()[..];
                if n == 0 {
                    return Err(tunnel_eof());
                } else if read.len() > 12 {
                    if read.starts_with(b"HTTP/1.1 200") || read.starts_with(b"HTTP/1.0 200") {
                        if read.ends_with(b"\r\n\r\n") {
                            return Ok(self.conn.take().unwrap().into());
                        }
                        // else read more
                    } else if read.starts_with(b"HTTP/1.1 407") {
                        return Err(io::Error::new(io::ErrorKind::Other, "proxy authentication required"));
                    } else if read.starts_with(b"HTTP/1.1 403") {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "proxy blocked this request",
                        ));
                    } else {
                        let (fst, _) = read.split_at(12);
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("unsuccessful tunnel: {:?}", fst).as_str(),
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(feature = "tls")]
#[inline]
fn tunnel_eof() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "unexpected eof while tunneling"
    )
}

#[cfg(feature = "default-tls")]
mod native_tls_async {
    use std::io::{self, Read, Write};

    use futures::{Poll, Future, Async};
    use native_tls::{self, HandshakeError, Error, TlsConnector};
    use tokio_io::{AsyncRead, AsyncWrite};

    /// A wrapper around an underlying raw stream which implements the TLS or SSL
    /// protocol.
    ///
    /// A `TlsStream<S>` represents a handshake that has been completed successfully
    /// and both the server and the client are ready for receiving and sending
    /// data. Bytes read from a `TlsStream` are decrypted from `S` and bytes written
    /// to a `TlsStream` are encrypted when passing through to `S`.
    #[derive(Debug)]
    pub struct TlsStream<S> {
        inner: native_tls::TlsStream<S>,
    }

    /// Future returned from `TlsConnectorExt::connect_async` which will resolve
    /// once the connection handshake has finished.
    pub struct ConnectAsync<S> {
        inner: MidHandshake<S>,
    }

    struct MidHandshake<S> {
        inner: Option<Result<native_tls::TlsStream<S>, HandshakeError<S>>>,
    }

    /// Extension trait for the `TlsConnector` type in the `native_tls` crate.
    pub trait TlsConnectorExt: sealed::Sealed {
        /// Connects the provided stream with this connector, assuming the provided
        /// domain.
        ///
        /// This function will internally call `TlsConnector::connect` to connect
        /// the stream and returns a future representing the resolution of the
        /// connection operation. The returned future will resolve to either
        /// `TlsStream<S>` or `Error` depending if it's successful or not.
        ///
        /// This is typically used for clients who have already established, for
        /// example, a TCP connection to a remote server. That stream is then
        /// provided here to perform the client half of a connection to a
        /// TLS-powered server.
        ///
        /// # Compatibility notes
        ///
        /// Note that this method currently requires `S: Read + Write` but it's
        /// highly recommended to ensure that the object implements the `AsyncRead`
        /// and `AsyncWrite` traits as well, otherwise this function will not work
        /// properly.
        fn connect_async<S>(&self, domain: &str, stream: S) -> ConnectAsync<S>
            where S: Read + Write; // TODO: change to AsyncRead + AsyncWrite
    }

    mod sealed {
        pub trait Sealed {}
    }

    impl<S: Read + Write> Read for TlsStream<S> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl<S: Read + Write> Write for TlsStream<S> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }


    impl<S: AsyncRead + AsyncWrite> AsyncRead for TlsStream<S> {
    }

    impl<S: AsyncRead + AsyncWrite> AsyncWrite for TlsStream<S> {
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            try_nb!(self.inner.shutdown());
            self.inner.get_mut().shutdown()
        }
    }

    impl TlsConnectorExt for TlsConnector {
        fn connect_async<S>(&self, domain: &str, stream: S) -> ConnectAsync<S>
            where S: Read + Write,
        {
            ConnectAsync {
                inner: MidHandshake {
                    inner: Some(self.connect(domain, stream)),
                },
            }
        }
    }

    impl sealed::Sealed for TlsConnector {}

    // TODO: change this to AsyncRead/AsyncWrite on next major version
    impl<S: Read + Write> Future for ConnectAsync<S> {
        type Item = TlsStream<S>;
        type Error = Error;

        fn poll(&mut self) -> Poll<TlsStream<S>, Error> {
            self.inner.poll()
        }
    }

    // TODO: change this to AsyncRead/AsyncWrite on next major version
    impl<S: Read + Write> Future for MidHandshake<S> {
        type Item = TlsStream<S>;
        type Error = Error;

        fn poll(&mut self) -> Poll<TlsStream<S>, Error> {
            match self.inner.take().expect("cannot poll MidHandshake twice") {
                Ok(stream) => Ok(TlsStream { inner: stream }.into()),
                Err(HandshakeError::Failure(e)) => Err(e),
                Err(HandshakeError::WouldBlock(s)) => {
                    match s.handshake() {
                        Ok(stream) => Ok(TlsStream { inner: stream }.into()),
                        Err(HandshakeError::Failure(e)) => Err(e),
                        Err(HandshakeError::WouldBlock(s)) => {
                            self.inner = Some(Err(HandshakeError::WouldBlock(s)));
                            Ok(Async::NotReady)
                        }
                    }
                }
            }
        }
    }
}


#[cfg(feature = "socks")]
mod socks {
    use std::io;

    use futures::{Future, future};
    use hyper::client::connect::{Connected, Destination};
    use socks::Socks5Stream;
    use std::net::ToSocketAddrs;
    use tokio::{net::TcpStream, reactor};

    use super::{Connecting};
    use proxy::{ProxyScheme};

    pub(super) enum DnsResolve {
        Local,
        Proxy,
    }

    pub(super) fn connect(
        proxy: ProxyScheme,
        dst: Destination,
        dns: DnsResolve,
    ) -> Connecting {
        let https = dst.scheme() == "https";
        let original_host = dst.host().to_owned();
        let mut host = original_host.clone();
        let port = dst.port().unwrap_or_else(|| {
            if https { 443 } else { 80 }
        });

        if let DnsResolve::Local = dns {
            let maybe_new_target = match (host.as_str(), port).to_socket_addrs() {
                Ok(mut iter) => iter.next(),
                Err(err) => {
                    return Box::new(future::err(err));
                }
            };
            if let Some(new_target) = maybe_new_target {
                host = new_target.ip().to_string();
            }
        }

        let (socket_addr, auth) = match proxy {
            ProxyScheme::Socks5 { addr, auth, .. } => (addr, auth),
            _ => unreachable!(),
        };

        // Get a Tokio TcpStream
        let stream = future::result(if let Some((username, password)) = auth {
            Socks5Stream::connect_with_password(
                socket_addr, (host.as_str(), port), &username, &password
            )
        } else {
            Socks5Stream::connect(socket_addr, (host.as_str(), port))
        }.and_then(|s| {
            TcpStream::from_std(s.into_inner(), &reactor::Handle::default())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }));

        Box::new(
            stream.map(|s| (Box::new(s) as super::Conn, Connected::new()))
        )
    }
}

#[cfg(feature = "tls")]
#[cfg(test)]
mod tests {
    extern crate tokio_tcp;

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use futures::Future;
    use tokio::runtime::current_thread::Runtime;
    use self::tokio_tcp::TcpStream;
    use super::tunnel;
    use proxy;

    static TUNNEL_UA: &'static str = "tunnel-test/x.y";
    static TUNNEL_OK: &'static [u8] = b"\
        HTTP/1.1 200 OK\r\n\
        \r\n\
    ";

    macro_rules! mock_tunnel {
        () => ({
            mock_tunnel!(TUNNEL_OK)
        });
        ($write:expr) => ({
            mock_tunnel!($write, "")
        });
        ($write:expr, $auth:expr) => ({
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let connect_expected = format!("\
                CONNECT {0}:{1} HTTP/1.1\r\n\
                Host: {0}:{1}\r\n\
                User-Agent: {2}\r\n\
                {3}\
                \r\n\
            ", addr.ip(), addr.port(), TUNNEL_UA, $auth).into_bytes();

            thread::spawn(move || {
                let (mut sock, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap();
                assert_eq!(&buf[..n], &connect_expected[..]);

                sock.write_all($write).unwrap();
            });
            addr
        })
    }

    #[test]
    fn test_tunnel() {
        let addr = mock_tunnel!();

        let mut rt = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port, TUNNEL_UA.parse().unwrap(), None)
        });

        rt.block_on(work).unwrap();
    }

    #[test]
    fn test_tunnel_eof() {
        let addr = mock_tunnel!(b"HTTP/1.1 200 OK");

        let mut rt = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port, TUNNEL_UA.parse().unwrap(), None)
        });

        rt.block_on(work).unwrap_err();
    }

    #[test]
    fn test_tunnel_non_http_response() {
        let addr = mock_tunnel!(b"foo bar baz hallo");

        let mut rt = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port, TUNNEL_UA.parse().unwrap(), None)
        });

        rt.block_on(work).unwrap_err();
    }

    #[test]
    fn test_tunnel_proxy_unauthorized() {
        let addr = mock_tunnel!(b"\
            HTTP/1.1 407 Proxy Authentication Required\r\n\
            Proxy-Authenticate: Basic realm=\"nope\"\r\n\
            \r\n\
        ");

        let mut rt = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port, TUNNEL_UA.parse().unwrap(), None)
        });

        let error = rt.block_on(work).unwrap_err();
        assert_eq!(error.to_string(), "proxy authentication required");
    }

    #[test]
    fn test_tunnel_basic_auth() {
        let addr = mock_tunnel!(
            TUNNEL_OK,
            "Proxy-Authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==\r\n"
        );

        let mut rt = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port, TUNNEL_UA.parse().unwrap(), Some(proxy::encode_basic_auth("Aladdin", "open sesame")))
        });

        rt.block_on(work).unwrap();
    }
}
