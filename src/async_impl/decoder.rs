/*!
A potentially non-blocking response decoder.

The decoder wraps a stream of chunks and produces a new stream of decompressed chunks.
The decompressed chunks aren't guaranteed to align to the compressed ones.

If the response is plaintext then no additional work is carried out.
Chunks are just passed along.

If the response is gzip, then the chunks are decompressed into a buffer.
Slices of that buffer are emitted as new chunks.
*/

use std::fmt;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};

use async_compression::stream::GzipDecoder;
use bytes::Bytes;
use futures_core::Stream;
use futures_util::stream::Peekable;
use hyper::header::{CONTENT_ENCODING, CONTENT_LENGTH, TRANSFER_ENCODING};
use hyper::HeaderMap;

use log::warn;

use super::Body;
use crate::error;

/// A response decompressor over a non-blocking stream of chunks.
///
/// The inner decoder may be constructed asynchronously.
pub(crate) struct Decoder {
    inner: Inner,
}

enum Inner {
    /// A `PlainText` decoder just returns the response content as is.
    PlainText(super::body::ImplStream),
    /// A `Gzip` decoder will uncompress the gzipped response content before returning it.
    Gzip(GzipDecoder<Peekable<IoStream>>),
    /// A decoder that doesn't have a value yet.
    Pending(Pending),
}

/// A future attempt to poll the response body for EOF so we know whether to use gzip or not.
struct Pending(Peekable<IoStream>);

struct IoStream(super::body::ImplStream);

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Decoder").finish()
    }
}

impl Decoder {
    /// An empty decoder.
    ///
    /// This decoder will produce a single 0 byte chunk.
    #[cfg(feature = "blocking")]
    pub(crate) fn empty() -> Decoder {
        Decoder {
            inner: Inner::PlainText(Body::empty().into_stream()),
        }
    }

    /// A plain text decoder.
    ///
    /// This decoder will emit the underlying chunks as-is.
    fn plain_text(body: Body) -> Decoder {
        Decoder {
            inner: Inner::PlainText(body.into_stream()),
        }
    }

    /// A gzip decoder.
    ///
    /// This decoder will buffer and decompress chunks that are gzipped.
    fn gzip(body: Body) -> Decoder {
        use futures_util::StreamExt;

        Decoder {
            inner: Inner::Pending(Pending(IoStream(body.into_stream()).peekable())),
        }
    }

    /// Constructs a Decoder from a hyper request.
    ///
    /// A decoder is just a wrapper around the hyper request that knows
    /// how to decode the content body of the request.
    ///
    /// Uses the correct variant by inspecting the Content-Encoding header.
    pub(crate) fn detect(headers: &mut HeaderMap, body: Body, check_gzip: bool) -> Decoder {
        if !check_gzip {
            return Decoder::plain_text(body);
        }
        let content_encoding_gzip: bool;
        let mut is_gzip = {
            content_encoding_gzip = headers
                .get_all(CONTENT_ENCODING)
                .iter()
                .any(|enc| enc == "gzip");
            content_encoding_gzip
                || headers
                    .get_all(TRANSFER_ENCODING)
                    .iter()
                    .any(|enc| enc == "gzip")
        };
        if is_gzip {
            if let Some(content_length) = headers.get(CONTENT_LENGTH) {
                if content_length == "0" {
                    warn!("gzip response with content-length of 0");
                    is_gzip = false;
                }
            }
        }
        if content_encoding_gzip {
            headers.remove(CONTENT_ENCODING);
            headers.remove(CONTENT_LENGTH);
        }
        if is_gzip {
            Decoder::gzip(body)
        } else {
            Decoder::plain_text(body)
        }
    }
}

impl Stream for Decoder {
    type Item = Result<Bytes, error::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        // Do a read or poll for a pending decoder value.
        let new_value = match self.inner {
            Inner::Pending(ref mut future) => match Pin::new(future).poll(cx) {
                Poll::Ready(Ok(inner)) => inner,
                Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(crate::error::decode_io(e)))),
                Poll::Pending => return Poll::Pending,
            },
            Inner::PlainText(ref mut body) => return Pin::new(body).poll_next(cx),
            Inner::Gzip(ref mut decoder) => {
                return match futures_core::ready!(Pin::new(decoder).poll_next(cx)) {
                    Some(Ok(bytes)) => Poll::Ready(Some(Ok(bytes))),
                    Some(Err(err)) => Poll::Ready(Some(Err(crate::error::decode_io(err)))),
                    None => Poll::Ready(None),
                }
            }
        };

        self.inner = new_value;
        self.poll_next(cx)
    }
}

impl Future for Pending {
    type Output = Result<Inner, std::io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use futures_util::StreamExt;

        match futures_core::ready!(Pin::new(&mut self.0).peek(cx)) {
            Some(Ok(_)) => {
                // fallthrough
            }
            Some(Err(_e)) => {
                // error was just a ref, so we need to really poll to move it
                return Poll::Ready(Err(futures_core::ready!(
                    Pin::new(&mut self.0).poll_next(cx)
                )
                .expect("just peeked Some")
                .unwrap_err()));
            }
            None => return Poll::Ready(Ok(Inner::PlainText(Body::empty().into_stream()))),
        };

        let body = mem::replace(
            &mut self.0,
            IoStream(Body::empty().into_stream()).peekable(),
        );
        Poll::Ready(Ok(Inner::Gzip(GzipDecoder::new(body))))
    }
}

impl Stream for IoStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match futures_core::ready!(Pin::new(&mut self.0).poll_next(cx)) {
            Some(Ok(chunk)) => Poll::Ready(Some(Ok(chunk))),
            Some(Err(err)) => Poll::Ready(Some(Err(err.into_io()))),
            None => Poll::Ready(None),
        }
    }
}
