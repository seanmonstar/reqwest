use std::io::{self, Read, Write};

use futures::{Poll, Future, Async};
use native_tls::{self, HandshakeError, Error, TlsConnector};
use tokio_io::{AsyncRead, AsyncWrite, try_nb};

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
