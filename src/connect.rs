use bytes::{BufMut, IntoBuf};
use futures::{Async, Future, Poll};
use hyper::client::{HttpConnector, Service};
use hyper::Uri;
use hyper_tls::{/*HttpsConnecting,*/ HttpsConnector, MaybeHttpsStream};
use native_tls::TlsConnector;
use tokio_core::reactor::Handle;
use tokio_io::{AsyncRead, AsyncWrite};

use std::io::{self, Cursor};
use std::sync::Arc;

use {proxy, Proxy};

// pub(crate)

pub struct Connector {
    https: HttpsConnector<HttpConnector>,
    proxies: Arc<Vec<Proxy>>,
}

impl Connector {
    pub fn new(tls: TlsConnector, proxies: Arc<Vec<Proxy>>, handle: &Handle) -> Connector {
        let mut http = HttpConnector::new(4, handle);
        http.enforce_http(false);
        let https = HttpsConnector::from((http, tls));

        Connector {
            https: https,
            proxies: proxies,
        }
    }
}

impl Service for Connector {
    type Request = Uri;
    type Response = Conn;
    type Error = io::Error;
    type Future = Connecting;

    fn call(&self, uri: Uri) -> Self::Future {
        for prox in self.proxies.iter() {
            if let Some(puri) = proxy::proxies(prox, &uri) {
                if uri.scheme() == Some("https") {
                    let host = uri.authority().unwrap().to_owned();
                    return Box::new(self.https.call(puri).and_then(|conn| {
                        tunnel(conn, host)
                    }));
                }
                return Box::new(self.https.call(puri));
            }
        }
        Box::new(self.https.call(uri))
    }
}

pub type Conn = MaybeHttpsStream<<HttpConnector as Service>::Response>;
pub type Connecting = Box<Future<Item=Conn, Error=io::Error>>;

fn tunnel<T>(conn: T, host: String) -> Tunnel<T> {
     let buf = format!("\
        CONNECT {0} HTTP/1.1\r\n\
        Host: {0}\r\n\
        \r\n\
    ", host).into_bytes();

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
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected eof while tunneling"));
                }
            } else {
                let n = try_ready!(self.conn.as_mut().unwrap().read_buf(&mut self.buf.get_mut()));
                let read = &self.buf.get_ref()[..];
                if n == 0 {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected eof while tunneling"));
                } else if read.len() > 12 {
                    if read.starts_with(b"HTTP/1.1 200") {
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
                CONNECT {0} HTTP/1.1\r\n\
                Host: {0}\r\n\
                \r\n\
            ", addr).into_bytes();

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
        let host = addr.to_string();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host)
        });

        core.run(work).unwrap();
    }

    #[test]
    fn test_tunnel_eof() {
        let addr = mock_tunnel!(b"HTTP/1.1 200 OK");

        let mut core = Core::new().unwrap();
        let work = TcpStream::connect(&addr, &core.handle());
        let host = addr.to_string();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host)
        });

        core.run(work).unwrap_err();
    }

    #[test]
    fn test_tunnel_bad_response() {
        let addr = mock_tunnel!(b"foo bar baz hallo");

        let mut core = Core::new().unwrap();
        let work = TcpStream::connect(&addr, &core.handle());
        let host = addr.to_string();
        let work = work.and_then(|tcp| {
            tunnel(tcp, host)
        });

        core.run(work).unwrap_err();
    }
}
