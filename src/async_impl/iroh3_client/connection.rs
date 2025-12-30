//! QUIC Transport implementation with Quinn
//!
//! This module implements QUIC traits with Quinn.
#![deny(missing_docs)]

use std::{
    convert::TryInto,
    fmt::{self, Display},
    future::Future,
    pin::Pin,
    task::{self, Poll},
};

use bytes::{Buf, Bytes};
use futures::{ready, stream, Stream, StreamExt};
use h3::quic::{self, ConnectionErrorIncoming, StreamErrorIncoming, StreamId, WriteBuf};
use iroh::endpoint::{self, ClosedStream, ReadDatagram};
pub use iroh::endpoint::{AcceptBi, AcceptUni, OpenBi, OpenUni, VarInt, WriteError};
use tokio_util::sync::ReusableBoxFuture;
/// BoxStream with Sync trait
type BoxStreamSync<'a, T> = Pin<Box<dyn Stream<Item = T> + Sync + Send + 'a>>;

/// A QUIC connection backed by Quinn
///
/// Implements a [`quic::Connection`] backed by a [`endpoint::Connection`].
pub struct Connection {
    conn: iroh::endpoint::Connection,
    incoming_bi: BoxStreamSync<'static, <AcceptBi<'static> as Future>::Output>,
    opening_bi: Option<BoxStreamSync<'static, <OpenBi<'static> as Future>::Output>>,
    incoming_uni: BoxStreamSync<'static, <AcceptUni<'static> as Future>::Output>,
    opening_uni: Option<BoxStreamSync<'static, <OpenUni<'static> as Future>::Output>>,
    #[allow(dead_code)]
    datagrams: BoxStreamSync<'static, <ReadDatagram<'static> as Future>::Output>,
}

impl Connection {
    /// Create a [`Connection`] from a [`endpoint::Connection`]
    pub fn new(conn: iroh::endpoint::Connection) -> Self {
        Self {
            conn: conn.clone(),
            incoming_bi: Box::pin(stream::unfold(conn.clone(), |conn| async {
                Some((conn.accept_bi().await, conn))
            })),
            opening_bi: None,
            incoming_uni: Box::pin(stream::unfold(conn.clone(), |conn| async {
                Some((conn.accept_uni().await, conn))
            })),
            opening_uni: None,
            datagrams: Box::pin(stream::unfold(conn, |conn| async {
                Some((conn.read_datagram().await, conn))
            })),
        }
    }
}

/// The error type for [`Connection`]
///
/// Wraps reasons a Quinn connection might be lost.
#[derive(Debug)]
pub struct ConnectionError(endpoint::ConnectionError);

impl std::error::Error for ConnectionError {}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<endpoint::ConnectionError> for ConnectionError {
    fn from(e: endpoint::ConnectionError) -> Self {
        Self(e)
    }
}

impl From<ConnectionError> for ConnectionErrorIncoming {
    fn from(value: ConnectionError) -> Self {
        ConnectionErrorIncoming::InternalError(value.to_string())
    }
}

impl From<ConnectionError> for StreamErrorIncoming {
    fn from(value: ConnectionError) -> Self {
        StreamErrorIncoming::ConnectionErrorIncoming {
            connection_error: value.into(),
        }
    }
}

impl<B> quic::Connection<B> for Connection
where
    B: Buf,
{
    type RecvStream = RecvStream;
    type OpenStreams = OpenStreams;

    fn poll_accept_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, ConnectionErrorIncoming>> {
        let (send, recv) = match ready!(self.incoming_bi.poll_next_unpin(cx)) {
            Some(x) => x.map_err(ConnectionError::from)?,
            None => {
                return Poll::Ready(Err(ConnectionErrorIncoming::InternalError(
                    "no connection".into(),
                )));
            }
        };
        Poll::Ready(Ok(Self::BidiStream {
            send: Self::SendStream::new(send),
            recv: Self::RecvStream::new(recv),
        }))
    }

    fn poll_accept_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::RecvStream, ConnectionErrorIncoming>> {
        let recv = match ready!(self.incoming_uni.poll_next_unpin(cx)) {
            Some(x) => x.map_err(ConnectionError::from)?,
            None => {
                return Poll::Ready(Err(ConnectionErrorIncoming::InternalError(
                    "no connection".into(),
                )));
            }
        };
        Poll::Ready(Ok(Self::RecvStream::new(recv)))
    }

    fn opener(&self) -> Self::OpenStreams {
        OpenStreams {
            conn: self.conn.clone(),
            opening_bi: None,
            opening_uni: None,
        }
    }
}

impl<B> quic::OpenStreams<B> for Connection
where
    B: Buf,
{
    type SendStream = SendStream<B>;
    type BidiStream = BidiStream<B>;

    fn poll_open_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, StreamErrorIncoming>> {
        if self.opening_bi.is_none() {
            self.opening_bi = Some(Box::pin(stream::unfold(self.conn.clone(), |conn| async {
                Some((conn.clone().open_bi().await, conn))
            })));
        }

        let (send, recv) = ready!(self.opening_bi.as_mut().unwrap().poll_next_unpin(cx))
            .unwrap()
            .map_err(ConnectionError::from)?;
        Poll::Ready(Ok(Self::BidiStream {
            send: Self::SendStream::new(send),
            recv: RecvStream::new(recv),
        }))
    }

    fn poll_open_send(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::SendStream, StreamErrorIncoming>> {
        if self.opening_uni.is_none() {
            self.opening_uni = Some(Box::pin(stream::unfold(self.conn.clone(), |conn| async {
                Some((conn.open_uni().await, conn))
            })));
        }

        let send = ready!(self.opening_uni.as_mut().unwrap().poll_next_unpin(cx))
            .unwrap()
            .map_err(ConnectionError::from)?;
        Poll::Ready(Ok(Self::SendStream::new(send)))
    }

    fn close(&mut self, code: h3::error::Code, reason: &[u8]) {
        self.conn.close(
            VarInt::from_u64(code.value()).expect("error code VarInt"),
            reason,
        );
    }
}

/// Stream opener backed by a Quinn connection
///
/// Implements [`quic::OpenStreams`] using [`endpoint::Connection`],
/// [`endpoint::OpenBi`], [`endpoint::OpenUni`].
pub struct OpenStreams {
    conn: endpoint::Connection,
    opening_bi: Option<BoxStreamSync<'static, <OpenBi<'static> as Future>::Output>>,
    opening_uni: Option<BoxStreamSync<'static, <OpenUni<'static> as Future>::Output>>,
}

impl<B> quic::OpenStreams<B> for OpenStreams
where
    B: Buf,
{
    type SendStream = SendStream<B>;
    type BidiStream = BidiStream<B>;

    fn poll_open_bidi(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::BidiStream, StreamErrorIncoming>> {
        if self.opening_bi.is_none() {
            self.opening_bi = Some(Box::pin(stream::unfold(self.conn.clone(), |conn| async {
                Some((conn.open_bi().await, conn))
            })));
        }

        let (send, recv) = ready!(self.opening_bi.as_mut().unwrap().poll_next_unpin(cx))
            .unwrap()
            .map_err(ConnectionError::from)?;
        Poll::Ready(Ok(Self::BidiStream {
            send: Self::SendStream::new(send),
            recv: RecvStream::new(recv),
        }))
    }

    fn poll_open_send(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Self::SendStream, StreamErrorIncoming>> {
        if self.opening_uni.is_none() {
            self.opening_uni = Some(Box::pin(stream::unfold(self.conn.clone(), |conn| async {
                Some((conn.open_uni().await, conn))
            })));
        }

        let send = ready!(self.opening_uni.as_mut().unwrap().poll_next_unpin(cx))
            .unwrap()
            .map_err(ConnectionError::from)?;
        Poll::Ready(Ok(Self::SendStream::new(send)))
    }

    fn close(&mut self, code: h3::error::Code, reason: &[u8]) {
        self.conn.close(
            VarInt::from_u64(code.value()).expect("error code VarInt"),
            reason,
        );
    }
}

impl Clone for OpenStreams {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            opening_bi: None,
            opening_uni: None,
        }
    }
}

/// Quinn-backed bidirectional stream
///
/// Implements [`quic::BidiStream`] which allows the stream to be split
/// into two structs each implementing one direction.
pub struct BidiStream<B>
where
    B: Buf,
{
    send: SendStream<B>,
    recv: RecvStream,
}

impl<B> quic::BidiStream<B> for BidiStream<B>
where
    B: Buf,
{
    type SendStream = SendStream<B>;
    type RecvStream = RecvStream;

    fn split(self) -> (Self::SendStream, Self::RecvStream) {
        (self.send, self.recv)
    }
}

impl<B: Buf> quic::RecvStream for BidiStream<B> {
    type Buf = Bytes;

    fn poll_data(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, StreamErrorIncoming>> {
        self.recv.poll_data(cx)
    }

    fn stop_sending(&mut self, error_code: u64) {
        self.recv.stop_sending(error_code)
    }

    fn recv_id(&self) -> StreamId {
        self.recv.recv_id()
    }
}

impl<B> quic::SendStream<B> for BidiStream<B>
where
    B: Buf,
{
    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        self.send.poll_ready(cx)
    }

    fn poll_finish(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        self.send.poll_finish(cx)
    }

    fn reset(&mut self, reset_code: u64) {
        self.send.reset(reset_code)
    }

    fn send_data<D: Into<WriteBuf<B>>>(&mut self, data: D) -> Result<(), StreamErrorIncoming> {
        self.send.send_data(data)
    }

    fn send_id(&self) -> StreamId {
        self.send.send_id()
    }
}
impl<B> quic::SendStreamUnframed<B> for BidiStream<B>
where
    B: Buf,
{
    fn poll_send<D: Buf>(
        &mut self,
        cx: &mut task::Context<'_>,
        buf: &mut D,
    ) -> Poll<Result<usize, StreamErrorIncoming>> {
        self.send.poll_send(cx, buf)
    }
}

/// Quinn-backed receive stream
///
/// Implements a [`quic::RecvStream`] backed by a [`endpoint::RecvStream`].
pub struct RecvStream {
    stream: Option<endpoint::RecvStream>,
    read_chunk_fut: ReadChunkFuture,
}

type ReadChunkFuture = ReusableBoxFuture<
    'static,
    (
        endpoint::RecvStream,
        Result<Option<endpoint::Chunk>, endpoint::ReadError>,
    ),
>;

impl RecvStream {
    fn new(stream: endpoint::RecvStream) -> Self {
        Self {
            stream: Some(stream),
            // Should only allocate once the first time it's used
            read_chunk_fut: ReusableBoxFuture::new(async { unreachable!() }),
        }
    }
}

impl quic::RecvStream for RecvStream {
    type Buf = Bytes;

    fn poll_data(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, StreamErrorIncoming>> {
        if let Some(mut stream) = self.stream.take() {
            self.read_chunk_fut.set(async move {
                let chunk = stream.read_chunk(usize::MAX, true).await;
                (stream, chunk)
            })
        };

        let (stream, chunk) = ready!(self.read_chunk_fut.poll(cx));
        self.stream = Some(stream);
        let chunk = chunk
            .map_err(|err| StreamErrorIncoming::Unknown(Box::new(err)))?
            .map(|c| c.bytes);
        Poll::Ready(Ok(chunk))
    }

    fn stop_sending(&mut self, error_code: u64) {
        self.stream
            .as_mut()
            .unwrap()
            .stop(VarInt::from_u64(error_code).expect("invalid error_code"))
            .ok();
    }

    fn recv_id(&self) -> StreamId {
        self.stream
            .as_ref()
            .unwrap()
            .id()
            .index()
            .try_into()
            .expect("invalid stream id")
    }
}

/// The error type for [`RecvStream`]
///
/// Wraps errors that occur when reading from a receive stream.
#[derive(Debug)]
pub struct ReadError(endpoint::ReadError);

impl From<ReadError> for std::io::Error {
    fn from(value: ReadError) -> Self {
        value.0.into()
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<endpoint::ReadError> for ReadError {
    fn from(e: endpoint::ReadError) -> Self {
        Self(e)
    }
}

/// Quinn-backed send stream
///
/// Implements a [`quic::SendStream`] backed by a [`endpoint::SendStream`].
pub struct SendStream<B: Buf> {
    stream: Option<endpoint::SendStream>,
    writing: Option<WriteBuf<B>>,
    write_fut: WriteFuture,
}

type WriteFuture =
    ReusableBoxFuture<'static, (endpoint::SendStream, Result<usize, endpoint::WriteError>)>;

impl<B> SendStream<B>
where
    B: Buf,
{
    fn new(stream: endpoint::SendStream) -> SendStream<B> {
        Self {
            stream: Some(stream),
            writing: None,
            write_fut: ReusableBoxFuture::new(async { unreachable!() }),
        }
    }
}

impl<B> quic::SendStream<B> for SendStream<B>
where
    B: Buf,
{
    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        if let Some(ref mut data) = self.writing {
            while data.has_remaining() {
                if let Some(mut stream) = self.stream.take() {
                    let chunk = data.chunk().to_owned(); // FIXME - avoid copy
                    self.write_fut.set(async move {
                        let ret = stream.write(&chunk).await;
                        (stream, ret)
                    });
                }

                let (stream, res) = ready!(self.write_fut.poll(cx));
                self.stream = Some(stream);
                match res {
                    Ok(cnt) => data.advance(cnt),
                    Err(err) => {
                        return Poll::Ready(Err(StreamErrorIncoming::Unknown(Box::new(err))));
                    }
                }
            }
        }
        self.writing = None;
        Poll::Ready(Ok(()))
    }

    fn poll_finish(
        &mut self,
        _cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), StreamErrorIncoming>> {
        Poll::Ready(
            self.stream
                .as_mut()
                .unwrap()
                .finish()
                .map_err(|e| StreamErrorIncoming::Unknown(Box::new(e))),
        )
    }

    fn reset(&mut self, reset_code: u64) {
        let _ = self
            .stream
            .as_mut()
            .unwrap()
            .reset(VarInt::from_u64(reset_code).unwrap_or(VarInt::MAX));
    }

    fn send_data<D: Into<WriteBuf<B>>>(&mut self, data: D) -> Result<(), StreamErrorIncoming> {
        if self.writing.is_some() {
            // TODO: what error?
            // return Err(StreamErrorIncoming::StreamTerminated);
        }
        self.writing = Some(data.into());
        Ok(())
    }

    fn send_id(&self) -> StreamId {
        self.stream
            .as_ref()
            .unwrap()
            .id()
            .index()
            .try_into()
            .expect("invalid stream id")
    }
}

impl<B> quic::SendStreamUnframed<B> for SendStream<B>
where
    B: Buf,
{
    fn poll_send<D: Buf>(
        &mut self,
        cx: &mut task::Context<'_>,
        buf: &mut D,
    ) -> Poll<Result<usize, StreamErrorIncoming>> {
        if self.writing.is_some() {
            // This signifies a bug in implementation
            panic!("poll_send called while send stream is not ready")
        }

        let s = Pin::new(self.stream.as_mut().unwrap());

        let res = ready!(s.poll_write(cx, buf.chunk()));
        match res {
            Ok(written) => {
                buf.advance(written);
                Poll::Ready(Ok(written))
            }
            Err(err) => Poll::Ready(Err(StreamErrorIncoming::Unknown(Box::new(err)))),
        }
    }
}

/// The error type for [`SendStream`]
///
/// Wraps errors that can happen writing to or polling a send stream.
#[derive(Debug)]
pub enum SendStreamError {
    /// Errors when writing, wrapping a [`endpoint::WriteError`]
    Write(WriteError),
    /// Error when the stream is not ready, because it is still sending
    /// data from a previous call
    NotReady,
    /// Error when the stream is closed
    StreamClosed(ClosedStream),
}

impl From<SendStreamError> for std::io::Error {
    fn from(value: SendStreamError) -> Self {
        match value {
            SendStreamError::Write(err) => err.into(),
            SendStreamError::NotReady => std::io::Error::other("send stream is not ready"),
            SendStreamError::StreamClosed(err) => err.into(),
        }
    }
}

impl std::error::Error for SendStreamError {}

impl Display for SendStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<WriteError> for SendStreamError {
    fn from(e: WriteError) -> Self {
        Self::Write(e)
    }
}

impl From<ClosedStream> for SendStreamError {
    fn from(value: ClosedStream) -> Self {
        Self::StreamClosed(value)
    }
}
