use hyper::client::HttpConnector;
use hyper::client::connect::Destination;
use hyper_tls::HttpsConnector;
use native_tls::TlsConnector;

use connect::Connecting;
use proxy::{ProxyScheme, ProxySchemeKind};



#[cfg(feature = "socks")]
mod socks {
    use std::io;

    use futures::{Future, future};
    use hyper::client::connect::{Connected, Destination};
    use native_tls::TlsConnector;
    use socks::Socks5Stream;
    use std::net::ToSocketAddrs;
    use tokio::{net, reactor};

    use connect_async::{TlsConnectorExt, TlsStream};
    use connect::{Connecting, Conn};
    use proxy::{ProxyScheme, ProxyAuth};

    pub fn connect(proxy: ProxyScheme, dst: Destination, tls: &TlsConnector, dns: bool) -> Connecting {
        let https = dst.scheme() == "https";
        let original_host = dst.host().to_owned();
        let mut host = original_host.clone();
        let port = dst.port().unwrap_or_else(|| {
            if https { 443 } else { 80 }
        });

        // If `dns` is not specified, we perform DNS resolution without using the proxy
        if !dns {
            let maybe_new_target = (host.as_str(), port).to_socket_addrs().unwrap().next();
            if let Some(new_target) = maybe_new_target {
                host = new_target.ip().to_string();
            }
        }

        let socket_addrs = &proxy.socket_addrs.unwrap()[..];

        // Get a Tokio TcpStream
        let stream = future::result(if let Some(auth) = proxy.auth {
            match auth {
                ProxyAuth::Basic {
                    username, password
                } => {
                    Socks5Stream::connect_with_password(
                        socket_addrs, (host.as_str(), port), &username, &password
                    )
                }
            }
        } else {
            Socks5Stream::connect(socket_addrs, (host.as_str(), port))
        }.and_then(|s| {
            net::TcpStream::from_std(s.into_inner(), &reactor::Handle::default())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }));

        // Add the TLS layer for HTTPS requests
        if https {
            let tls = tls.clone();
            Box::new(stream.and_then(move |s: net::TcpStream| {
                tls.connect_async(&original_host, s)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            }).map(|s: TlsStream<net::TcpStream>| {
                (Conn::Proxied(Box::new(s)), Connected::new())
            }))
        } else {
            Box::new(
                stream.map(|s| (Conn::Proxied(Box::new(s)), Connected::new()))
            )
        }
    }
}

#[cfg(not(feature = "socks"))]
mod socks {
    use std::io;

    use futures::future;
    use hyper::client::connect::Destination;
    use native_tls::TlsConnector;

    use connect::Connecting;
    use proxy::ProxyScheme;

    pub fn connect(_proxy: ProxyScheme, _dst: Destination, _tls: &TlsConnector, _dns: bool) -> Connecting {
        Box::new(future::err(
            io::Error::new(io::ErrorKind::Other, "Attempted to use SOCKS proxy when socks feature not enabled in reqwest")
        ))
    }
}

mod http {
    use std::io::{self, Cursor};

    use bytes::{BufMut, IntoBuf};
    use futures::{Async, Future, Poll};
    use http::uri::Scheme;
    use hyper::client::HttpConnector;
    use hyper::client::connect::{Connect, Destination};
    use hyper_tls::HttpsConnector;
    use native_tls::TlsConnector;
    use tokio_io::{AsyncRead, AsyncWrite};

    use connect_async::TlsConnectorExt;
    use connect::{Connecting, Conn};
    use proxy::ProxyScheme;

    pub fn connect(proxy: ProxyScheme, dst: Destination, tls: &TlsConnector, https: &HttpsConnector<HttpConnector>) -> Connecting {
        let puri = proxy.uri.unwrap();
        let mut ndst = dst.clone();
        let new_scheme = puri
            .scheme_part()
            .map(Scheme::as_str)
            .unwrap_or("http");
        ndst.set_scheme(new_scheme)
            .expect("proxy target scheme should be valid");

        ndst.set_host(puri.host().expect("proxy target should have host"))
            .expect("proxy target host should be valid");

        ndst.set_port(puri.port());

        if dst.scheme() == "https" {
            let host = dst.host().to_owned();
            let port = dst.port().unwrap_or(443);
            let tls = tls.clone();
            return Box::new(https.connect(ndst).and_then(move |(conn, connected)| {
                trace!("tunneling HTTPS over proxy");
                tunnel(conn, host.clone(), port)
                    .and_then(move |tunneled| {
                        tls.connect_async(&host, tunneled)
                            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                    })
                    .map(|io| (Conn::Proxied(Box::new(io)), connected.proxy(true)))
            }));
        }
        return Box::new(https.connect(ndst).map(|(io, connected)| (Conn::Normal(io), connected.proxy(true))));
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
}

pub fn connect(proxy: ProxyScheme, dst: Destination, tls: &TlsConnector, https: &HttpsConnector<HttpConnector>) -> Connecting {
    match proxy.kind {
        ProxySchemeKind::Http | ProxySchemeKind::Https => http::connect(proxy, dst, tls, https),
        ProxySchemeKind::Socks5 => socks::connect(proxy, dst, tls, false),
        ProxySchemeKind::Socks5h => socks::connect(proxy, dst, tls, true),
    }
}
