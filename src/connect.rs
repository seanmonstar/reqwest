use bytes::{Buf, BufMut};
use futures::{Future, Poll};
use hyper::client::{HttpConnector};
use hyper::client::connect::{Connect, Connected, Destination};
use hyper_tls::{HttpsConnector, MaybeHttpsStream};
use native_tls::TlsConnector;
use tokio_io::{AsyncRead, AsyncWrite};

use std::io::{self, Read, Write};
use std::sync::Arc;

use {Proxy, proxy_connect};

pub(crate) struct Connector {
    https: HttpsConnector<HttpConnector>,
    proxies: Arc<Vec<Proxy>>,
    tls: TlsConnector,
}

impl Connector {
    pub(crate) fn new(threads: usize, tls: TlsConnector, proxies: Arc<Vec<Proxy>>) -> Connector {
        let mut http = HttpConnector::new(threads);
        http.enforce_http(false);
        let https = HttpsConnector::from((http, tls.clone()));

        Connector {
            https: https,
            proxies: proxies,
            tls: tls,
        }
    }
}

impl Connect for Connector {
    type Transport = Conn;
    type Error = io::Error;
    type Future = Connecting;

    fn connect(&self, dst: Destination) -> Self::Future {
        for prox in self.proxies.iter() {
            if let Some(proxy_scheme_result) = prox.intercept(&dst) {
                let proxy_scheme = proxy_scheme_result.unwrap();
                trace!("proxy({:?}) intercepts {:?}", proxy_scheme, dst);

                return proxy_connect::connect(proxy_scheme, dst, &self.tls, &self.https);
            }
        }
        Box::new(self.https.connect(dst).map(|(io, connected)| (Conn::Normal(io), connected)))
    }
}

type HttpStream = <HttpConnector as Connect>::Transport;
type HttpsStream = MaybeHttpsStream<HttpStream>;

pub(crate) type Connecting = Box<Future<Item=(Conn, Connected), Error=io::Error> + Send>;

pub trait StreamLike: Read + Write + AsyncRead + AsyncWrite + Send {}
impl<T: Read + Write + AsyncRead + AsyncWrite + Send> StreamLike for T {}

type ProxyStream = Box<StreamLike>;

pub(crate) enum Conn {
    Normal(HttpsStream),
    Proxied(ProxyStream),
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
