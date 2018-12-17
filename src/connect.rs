use futures::Future;
use http::uri::Scheme;
use hyper::client::connect::{Connect, Connected, Destination};
use tokio_io::{AsyncRead, AsyncWrite};

#[cfg(feature = "default-tls")]
use native_tls::{TlsConnector, TlsConnectorBuilder};
#[cfg(feature = "tls")]
use futures::Poll;
#[cfg(feature = "tls")]
use bytes::BufMut;

use std::io;
use std::sync::Arc;

use dns::TrustDnsResolver;
use Proxy;

type HttpConnector = ::hyper::client::HttpConnector<TrustDnsResolver>;


pub(crate) struct Connector {
    proxies: Arc<Vec<Proxy>>,
    inner: Inner
}

enum Inner {
    #[cfg(not(feature = "tls"))]
    Http(HttpConnector),
    #[cfg(feature = "default-tls")]
    DefaultTls(::hyper_tls::HttpsConnector<HttpConnector>, TlsConnector),
    #[cfg(feature = "rustls-tls")]
    RustlsTls(::hyper_rustls::HttpsConnector<HttpConnector>, Arc<rustls::ClientConfig>)
}

impl Connector {
    #[cfg(not(feature = "tls"))]
    pub(crate) fn new(proxies: Arc<Vec<Proxy>>) -> Connector {
        let http = http_connector();
        Connector {
            proxies,
            inner: Inner::Http(http)
        }
    }

    #[cfg(feature = "default-tls")]
    pub(crate) fn new_default_tls(tls: TlsConnectorBuilder, proxies: Arc<Vec<Proxy>>) -> ::Result<Connector> {
        let tls = try_!(tls.build());

        let mut http = http_connector();
        http.enforce_http(false);
        let http = ::hyper_tls::HttpsConnector::from((http, tls.clone()));

        Ok(Connector {
            proxies,
            inner: Inner::DefaultTls(http, tls)
        })
    }

    #[cfg(feature = "rustls-tls")]
    pub(crate) fn new_rustls_tls(tls: rustls::ClientConfig, proxies: Arc<Vec<Proxy>>) -> ::Result<Connector> {
        let mut http = http_connector();
        http.enforce_http(false);
        let http = ::hyper_rustls::HttpsConnector::from((http, tls.clone()));

        Ok(Connector {
            proxies,
            inner: Inner::RustlsTls(http, Arc::new(tls))
        })
    }
}

fn http_connector() -> HttpConnector {
    let http = HttpConnector::new_with_resolver(TrustDnsResolver::new());
    http
}

impl Connect for Connector {
    type Transport = Conn;
    type Error = io::Error;
    type Future = Connecting;

    fn connect(&self, dst: Destination) -> Self::Future {
        macro_rules! connect {
            ( $http:expr, $dst:expr, $proxy:expr ) => {
                Box::new($http.connect($dst)
                    .map(|(io, connected)| (Box::new(io) as Conn, connected.proxy($proxy))))
            };
            ( $dst:expr, $proxy:expr ) => {
                match &self.inner {
                    #[cfg(not(feature = "tls"))]
                    Inner::Http(http) => connect!(http, $dst, $proxy),
                    #[cfg(feature = "default-tls")]
                    Inner::DefaultTls(http, _) => connect!(http, $dst, $proxy),
                    #[cfg(feature = "rustls-tls")]
                    Inner::RustlsTls(http, _) => connect!(http, $dst, $proxy)
                }
            };
        }

        for prox in self.proxies.iter() {
            if let Some(puri) = prox.intercept(&dst) {
                trace!("proxy({:?}) intercepts {:?}", puri, dst);
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

                match &self.inner {
                    #[cfg(feature = "default-tls")]
                    Inner::DefaultTls(http, tls) => if dst.scheme() == "https" {
                        #[cfg(feature = "default-tls")]
                        use connect_async::TlsConnectorExt;

                        let host = dst.host().to_owned();
                        let port = dst.port().unwrap_or(443);
                        let tls = tls.clone();
                        return Box::new(http.connect(ndst).and_then(move |(conn, connected)| {
                            trace!("tunneling HTTPS over proxy");
                            tunnel(conn, host.clone(), port)
                                .and_then(move |tunneled| {
                                    tls.connect_async(&host, tunneled)
                                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                                })
                                .map(|io| (Box::new(io) as Conn, connected.proxy(true)))
                        }));
                    },
                    #[cfg(feature = "rustls-tls")]
                    Inner::RustlsTls(http, tls) => if dst.scheme() == "https" {
                        #[cfg(feature = "rustls-tls")]
                        use tokio_rustls::TlsConnector as RustlsConnector;
                        #[cfg(feature = "rustls-tls")]
                        use tokio_rustls::webpki::DNSNameRef;

                        let host = dst.host().to_owned();
                        let port = dst.port().unwrap_or(443);
                        let tls = tls.clone();
                        return Box::new(http.connect(ndst).and_then(move |(conn, connected)| {
                            trace!("tunneling HTTPS over proxy");
                            let maybe_dnsname = DNSNameRef::try_from_ascii_str(&host)
                                .map(|dnsname| dnsname.to_owned())
                                .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid DNS Name"));
                            tunnel(conn, host, port)
                                .and_then(move |tunneled| Ok((maybe_dnsname?, tunneled)))
                                .and_then(move |(dnsname, tunneled)| {
                                    RustlsConnector::from(tls).connect(dnsname.as_ref(), tunneled)
                                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                                })
                                .map(|io| (Box::new(io) as Conn, connected.proxy(true)))
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

pub(crate) type Connecting = Box<Future<Item=(Conn, Connected), Error=io::Error> + Send>;

#[cfg(feature = "tls")]
fn tunnel<T>(conn: T, host: String, port: u16) -> Tunnel<T> {
     let buf = format!("\
        CONNECT {0}:{1} HTTP/1.1\r\n\
        Host: {0}:{1}\r\n\
        \r\n\
    ", host, port).into_bytes();

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
where T: AsyncRead + AsyncWrite {
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
                    } else {
                        return Err(io::Error::new(io::ErrorKind::Other, "unsuccessful tunnel"));
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

#[cfg(feature = "tls")]
#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use futures::Future;
    use tokio::runtime::current_thread::Runtime;
    use tokio::net::TcpStream;
    use super::tunnel;


    macro_rules! mock_tunnel {
        () => ({
            mock_tunnel!(b"\
                HTTP/1.1 200 OK\r\n\
                \r\n\
            ")
        });
        ($write:expr) => ({
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let connect_expected = format!("\
                CONNECT {0}:{1} HTTP/1.1\r\n\
                Host: {0}:{1}\r\n\
                \r\n\
            ", addr.ip(), addr.port()).into_bytes();

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
            tunnel(tcp, host, port)
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
            tunnel(tcp, host, port)
        });

        rt.block_on(work).unwrap_err();
    }

    #[test]
    fn test_tunnel_bad_response() {
        let addr = mock_tunnel!(b"foo bar baz hallo");

        let mut rt = Runtime::new().unwrap();
        let work = TcpStream::connect(&addr);
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port)
        });

        rt.block_on(work).unwrap_err();
    }
}
