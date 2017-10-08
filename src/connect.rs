use bytes::{Buf, BufMut, IntoBuf};
use futures::{Async, Future, Poll};
use hyper::client::{HttpConnector, Service};
use hyper::Uri;
use hyper_tls::{HttpsConnector, MaybeHttpsStream};
use native_tls::TlsConnector;
use tokio_core::reactor::Handle;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tls::{TlsConnectorExt, TlsStream};

use std::io::{self, Cursor, Read, Write};
use std::sync::Arc;

use {proxy, Proxy};

// pub(crate)

pub struct Connector {
    https: HttpsConnector<HttpConnector>,
    proxies: Arc<Vec<Proxy>>,
    tls: TlsConnector,
}

impl Connector {
    pub fn new(threads: usize, tls: TlsConnector, proxies: Arc<Vec<Proxy>>, handle: &Handle) -> Connector {
        let mut http = HttpConnector::new(threads, handle);
        http.enforce_http(false);
        let https = HttpsConnector::from((http, tls.clone()));

        Connector {
            https: https,
            proxies: proxies,
            tls: tls,
        }
    }

    pub fn danger_disable_hostname_verification(&mut self) {
        self.https.danger_disable_hostname_verification(true);
    }
}

impl Service for Connector {
    type Request = Uri;
    type Response = Conn;
    type Error = io::Error;
    type Future = Connecting;

    fn call(&self, uri: Uri) -> Self::Future {
        for prox in self.proxies.iter() {
            if let Some(puri) = proxy::intercept(prox, &uri) {
                trace!("proxy({:?}) intercepts {:?}", puri, uri);
                if uri.scheme() == Some("https") {
                    let host = uri.host().unwrap().to_owned();
                    let port = uri.port().unwrap_or(443);
                    let tls = self.tls.clone();
                    return Box::new(self.https.call(puri).and_then(move |conn| {
                        trace!("tunneling HTTPS over proxy");
                        tunnel(conn, host.clone(), port)
                            .and_then(move |tunneled| {
                                tls.connect_async(&host, tunneled)
                                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                            })
                            .map(|io| Conn::Proxied(io))
                    }));
                }
                return Box::new(self.https.call(puri).map(|io| Conn::Normal(io)));
            }
        }
        Box::new(self.https.call(uri).map(|io| Conn::Normal(io)))
    }
}

type HttpStream = <HttpConnector as Service>::Response;
type HttpsStream = MaybeHttpsStream<HttpStream>;

pub type Connecting = Box<Future<Item=Conn, Error=io::Error>>;

pub enum Conn {
    Normal(HttpsStream),
    Proxied(TlsStream<MaybeHttpsStream<HttpStream>>),
}

impl Read for Conn {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            Conn::Normal(ref mut s) => s.read(buf),
            Conn::Proxied(ref mut s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match *self {
            Conn::Normal(ref mut s) => s.write(buf),
            Conn::Proxied(ref mut s) => s.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        match *self {
            Conn::Normal(ref mut s) => s.flush(),
            Conn::Proxied(ref mut s) => s.flush(),
        }
    }
}

impl AsyncRead for Conn {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match *self {
            Conn::Normal(ref s) => s.prepare_uninitialized_buffer(buf),
            Conn::Proxied(ref s) => s.prepare_uninitialized_buffer(buf),
        }
    }

    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match *self {
            Conn::Normal(ref mut s) => s.read_buf(buf),
            Conn::Proxied(ref mut s) => s.read_buf(buf),
        }
    }
}

impl AsyncWrite for Conn {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        match *self {
            Conn::Normal(ref mut s) => s.shutdown(),
            Conn::Proxied(ref mut s) => s.shutdown(),
        }
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match *self {
            Conn::Normal(ref mut s) => s.write_buf(buf),
            Conn::Proxied(ref mut s) => s.write_buf(buf),
        }
    }
}

fn tunnel<T>(conn: T, host: String, port: u16) -> Tunnel<T> {
     let buf = format!("\
        CONNECT {0}:{1} HTTP/1.1\r\n\
        Host: {0}:{1}\r\n\
        \r\n\
    ", host, port).into_bytes();

     Tunnel {
        buf: buf.into_buf(),
        conn: Some(conn),
        state: TunnelState::Writing,
     }
}

struct Tunnel<T> {
    buf: Cursor<Vec<u8>>,
    conn: Option<T>,
    state: TunnelState,
}

enum TunnelState {
    Writing,
    Reading
}

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
                            return Ok(Async::Ready(self.conn.take().unwrap()));
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

#[inline]
fn tunnel_eof() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "unexpected eof while tunneling"
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use futures::Future;
    use tokio_core::reactor::Core;
    use tokio_core::net::TcpStream;
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

        let mut core = Core::new().unwrap();
        let work = TcpStream::connect(&addr, &core.handle());
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port)
        });

        core.run(work).unwrap();
    }

    #[test]
    fn test_tunnel_eof() {
        let addr = mock_tunnel!(b"HTTP/1.1 200 OK");

        let mut core = Core::new().unwrap();
        let work = TcpStream::connect(&addr, &core.handle());
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port)
        });

        core.run(work).unwrap_err();
    }

    #[test]
    fn test_tunnel_bad_response() {
        let addr = mock_tunnel!(b"foo bar baz hallo");

        let mut core = Core::new().unwrap();
        let work = TcpStream::connect(&addr, &core.handle());
        let host = addr.ip().to_string();
        let port = addr.port();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host, port)
        });

        core.run(work).unwrap_err();
    }
}
