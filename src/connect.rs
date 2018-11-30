use bytes::{Buf, BufMut};
use futures::{Future, Poll};
use http::uri::Scheme;
use hyper::client::{HttpConnector};
use hyper::client::connect::{Connect, Connected, Destination};
#[cfg(feature = "default-tls")]
use hyper_tls::{HttpsConnector, MaybeHttpsStream};
#[cfg(feature = "default-tls")]
use native_tls::TlsConnector;
use tokio_io::{AsyncRead, AsyncWrite};
#[cfg(feature = "default-tls")]
use connect_async::{TlsConnectorExt, TlsStream};
#[cfg(feature = "rustls-tls")]
use hyper_rustls::{HttpsConnector, MaybeHttpsStream};
#[cfg(feature = "rustls-tls")]
use tokio_rustls::{TlsConnector as RustlsConnector, TlsStream};
#[cfg(feature = "rustls-tls")]
use tokio_rustls::webpki::DNSNameRef;

use std::io::{self, Read, Write};
use std::sync::Arc;

use Proxy;

pub(crate) struct Connector {
    #[cfg(feature = "tls")]
    http: HttpsConnector<HttpConnector>,
    #[cfg(not(feature = "tls"))]
    http: HttpConnector,
    proxies: Arc<Vec<Proxy>>,
    #[cfg(feature = "default-tls")]
    tls: TlsConnector,
    #[cfg(feature = "rustls-tls")]
    tls: Arc<rustls::ClientConfig>
}

impl Connector {
    #[cfg(not(feature = "tls"))]
    pub(crate) fn new(threads: usize, proxies: Arc<Vec<Proxy>>) -> Connector {
        let http = HttpConnector::new(threads);
        Connector {
            http,
            proxies,
        }
    }
    #[cfg(feature = "default-tls")]
    pub(crate) fn new(threads: usize, tls: TlsConnector, proxies: Arc<Vec<Proxy>>) -> Connector {
        let mut http = HttpConnector::new(threads);
        http.enforce_http(false);
        let http = HttpsConnector::from((http, tls.clone()));

        Connector {
            http,
            proxies,
            tls,
        }
    }

    #[cfg(feature = "rustls-tls")]
    pub(crate) fn new(threads: usize, tls: rustls::ClientConfig, proxies: Arc<Vec<Proxy>>) -> Connector {
        let mut http = HttpConnector::new(threads);
        http.enforce_http(false);
        let http = HttpsConnector::from((http, tls.clone()));

        Connector {
            http,
            proxies,
            tls: Arc::new(tls),
        }
    }
}

impl Connect for Connector {
    type Transport = Conn;
    type Error = io::Error;
    type Future = Connecting;

    fn connect(&self, dst: Destination) -> Self::Future {
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

                #[cfg(feature = "default-tls")]
                {
                if dst.scheme() == "https" {
                    let host = dst.host().to_owned();
                    let port = dst.port().unwrap_or(443);
                    let tls = self.tls.clone();
                    return Box::new(self.http.connect(ndst).and_then(move |(conn, connected)| {
                        trace!("tunneling HTTPS over proxy");
                        tunnel(conn, host.clone(), port)
                            .and_then(move |tunneled| {
                                tls.connect_async(&host, tunneled)
                                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                            })
                            .map(|io| (Conn::Proxied(io), connected.proxy(true)))
                    }));
                }
                }

                #[cfg(feature = "rustls-tls")]
                {
                if dst.scheme() == "https" {
                    let host = dst.host().to_owned();
                    let port = dst.port().unwrap_or(443);
                    let tls = self.tls.clone();
                    return Box::new(self.http.connect(ndst).and_then(move |(conn, connected)| {
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
                            .map(|io| (Conn::Proxied(io), connected.proxy(true)))
                    }));
                }
                }

                return Box::new(self.http.connect(ndst).map(|(io, connected)| (Conn::Normal(io), connected.proxy(true))));
            }
        }
        Box::new(self.http.connect(dst).map(|(io, connected)| (Conn::Normal(io), connected)))
    }
}

type HttpStream = <HttpConnector as Connect>::Transport;
#[cfg(feature = "tls")]
type HttpsStream = MaybeHttpsStream<HttpStream>;


pub(crate) type Connecting = Box<Future<Item=(Conn, Connected), Error=io::Error> + Send>;

pub(crate) enum Conn {
    #[cfg(feature = "tls")]
    Normal(HttpsStream),
    #[cfg(not(feature = "tls"))]
    Normal(HttpStream),
    #[cfg(feature = "default-tls")]
    Proxied(TlsStream<MaybeHttpsStream<HttpStream>>),
    #[cfg(feature = "rustls-tls")]
    Proxied(TlsStream<MaybeHttpsStream<HttpStream>, rustls::ClientSession>),
}

impl Read for Conn {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            Conn::Normal(ref mut s) => s.read(buf),
            #[cfg(feature = "tls")]
            Conn::Proxied(ref mut s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match *self {
            Conn::Normal(ref mut s) => s.write(buf),
            #[cfg(feature = "tls")]
            Conn::Proxied(ref mut s) => s.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        match *self {
            Conn::Normal(ref mut s) => s.flush(),
            #[cfg(feature = "tls")]
            Conn::Proxied(ref mut s) => s.flush(),
        }
    }
}

impl AsyncRead for Conn {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match *self {
            Conn::Normal(ref s) => s.prepare_uninitialized_buffer(buf),
            #[cfg(feature = "tls")]
            Conn::Proxied(ref s) => s.prepare_uninitialized_buffer(buf),
        }
    }

    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match *self {
            Conn::Normal(ref mut s) => s.read_buf(buf),
            #[cfg(feature = "tls")]
            Conn::Proxied(ref mut s) => s.read_buf(buf),
        }
    }
}

impl AsyncWrite for Conn {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match *self {
            Conn::Normal(ref mut s) => s.shutdown(),
            #[cfg(feature = "tls")]
            Conn::Proxied(ref mut s) => s.shutdown(),
        }
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match *self {
            Conn::Normal(ref mut s) => s.write_buf(buf),
            #[cfg(feature = "tls")]
            Conn::Proxied(ref mut s) => s.write_buf(buf),
        }
    }
}

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
