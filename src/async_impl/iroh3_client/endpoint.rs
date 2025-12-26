//! QuinnEndpoint is a wrapper around a quinn::SendStream and quinn::RecvStream
//!
//! It implements AsyncRead and AsyncWrite so it can be used with tokio::io::copy
use iroh::endpoint::{RecvStream, SendStream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub struct QuinnEndpoint {
    pub send: SendStream,
    pub recv: RecvStream,
}

impl AsyncRead for QuinnEndpoint {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<std::io::Result<()>> {
        let self_mut = self.get_mut();
        Pin::new(&mut self_mut.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for QuinnEndpoint {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let self_mut = self.get_mut();
        let send_poll = Pin::new(&mut self_mut.send).poll_write(cx, buf);
        send_poll.map_err(Into::into)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        let self_mut = self.get_mut();
        let flush_poll = Pin::new(&mut self_mut.send).poll_flush(cx);
        flush_poll.map_err(Into::into)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let self_mut = self.get_mut();
        let shutdown_poll = Pin::new(&mut self_mut.send).poll_shutdown(cx);
        shutdown_poll.map_err(Into::into)
    }
}
