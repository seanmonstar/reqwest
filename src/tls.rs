use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::time::Duration;
use std::fmt;

use hyper::net::{SslClient, HttpStream, NetworkStream};
use native_tls::{TlsConnector, TlsStream as NativeTlsStream, HandshakeError};

pub struct TlsClient(TlsConnector);

impl TlsClient {
    pub fn new() -> ::Result<TlsClient> {
        TlsConnector::builder()
            .and_then(|c| c.build())
            .map(TlsClient)
            .map_err(|e| ::Error::Http(::hyper::Error::Ssl(Box::new(e))))
    }
}

impl SslClient for TlsClient {
    type Stream = TlsStream;

    fn wrap_client(&self, stream: HttpStream, host: &str) -> ::hyper::Result<Self::Stream> {
        self.0.connect(host, stream).map(TlsStream).map_err(|e| {
            match e {
                HandshakeError::Failure(e) => ::hyper::Error::Ssl(Box::new(e)),
                HandshakeError::Interrupted(..) => {
                    // while using hyper 0.9, this won't happen, because the
                    // socket is in blocking mode. once we move to hyper 0.10,
                    // much of this `tls` module will go away anyways
                    unreachable!("TlsClient::handshake Interrupted")
                }
            }
        })
    }
}

impl fmt::Debug for TlsClient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("TlsClient").field(&"_").finish()
    }
}

#[derive(Debug)]
pub struct TlsStream(NativeTlsStream<HttpStream>);

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for TlsStream {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Clone for TlsStream {
    fn clone(&self) -> TlsStream {
        unreachable!("TlsStream::clone is never used for the Client")
    }
}

impl NetworkStream for TlsStream {
    fn peer_addr(&mut self) -> io::Result<SocketAddr> {
        self.0.get_mut().peer_addr()
    }

    fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.0.get_ref().set_read_timeout(dur)
    }

    fn set_write_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.0.get_ref().set_write_timeout(dur)
    }
}
