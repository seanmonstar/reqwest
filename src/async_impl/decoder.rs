/*!
A potentially non-blocking response decoder.

The decoder wraps a stream of chunks and produces a new stream of decompressed chunks.
The decompressed chunks aren't guaranteed to align to the compressed ones.

If the response is plaintext then no additional work is carried out.
Chunks are just passed along.

If the response is gzip, then the chunks are decompressed into a buffer.
Slices of that buffer are emitted as new chunks.

This module consists of a few main types:

- `ReadableChunks` is a `Read`-like wrapper around a stream
- `Decoder` is a layer over `ReadableChunks` that applies the right decompression

The following types directly support the gzip compression case:

- `Pending` is a non-blocking constructor for a `Decoder` in case the body needs to be checked for EOF
*/

use std::fmt;
use std::mem;
use std::cmp;
use std::io::{self, Read};

use bytes::{Buf, BufMut, BytesMut};
use flate2::read::GzDecoder;
use futures::{Async, Future, Poll, Stream};
use hyper::{HeaderMap};
use hyper::header::{CONTENT_ENCODING, CONTENT_LENGTH, TRANSFER_ENCODING};

use super::{Body, Chunk};
use error;

const INIT_BUFFER_SIZE: usize = 8192;

/// A response decompressor over a non-blocking stream of chunks.
///
/// The inner decoder may be constructed asynchronously.
pub struct Decoder {
    inner: Inner
}

enum Inner {
    /// A `PlainText` decoder just returns the response content as is.
    PlainText(Body),
    /// A `Gzip` decoder will uncompress the gzipped response content before returning it.
    Gzip(Gzip),
    /// A decoder that doesn't have a value yet.
    Pending(Pending)
}

/// A future attempt to poll the response body for EOF so we know whether to use gzip or not.
struct Pending {
    body: ReadableChunks<Body>,
}

/// A gzip decoder that reads from a `flate2::read::GzDecoder` into a `BytesMut` and emits the results
/// as a `Chunk`.
struct Gzip {
    inner: Box<GzDecoder<ReadableChunks<Body>>>,
    buf: BytesMut,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Decoder")
            .finish()
    }
}

impl Decoder {
    /// An empty decoder.
    ///
    /// This decoder will produce a single 0 byte chunk.
    #[inline]
    pub fn empty() -> Decoder {
        Decoder {
            inner: Inner::PlainText(Body::empty())
        }
    }

    /// A plain text decoder.
    ///
    /// This decoder will emit the underlying chunks as-is.
    #[inline]
    fn plain_text(body: Body) -> Decoder {
        Decoder {
            inner: Inner::PlainText(body)
        }
    }

    /// A gzip decoder.
    ///
    /// This decoder will buffer and decompress chunks that are gzipped.
    #[inline]
    fn gzip(body: Body) -> Decoder {
        Decoder {
            inner: Inner::Pending(Pending { body: ReadableChunks::new(body) })
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
                .fold(false, |acc, enc| acc || enc == "gzip");
            content_encoding_gzip ||
            headers
                .get_all(TRANSFER_ENCODING)
                .iter()
                .fold(false, |acc, enc| acc || enc == "gzip")
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
    type Item = Chunk;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Do a read or poll for a pendidng decoder value.
        let new_value = match self.inner {
            Inner::Pending(ref mut future) => {
                match future.poll() {
                    Ok(Async::Ready(inner)) => inner,
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(e) => return Err(e)
                }
            },
            Inner::PlainText(ref mut body) => return body.poll(),
            Inner::Gzip(ref mut decoder) => return decoder.poll()
        };

        self.inner = new_value;
        self.poll()
    }
}

impl Future for Pending {
    type Item = Inner;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let body_state = match self.body.poll_stream() {
            Ok(Async::Ready(state)) => state,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => return Err(e)
        };

        let body = mem::replace(&mut self.body, ReadableChunks::new(Body::empty()));
        match body_state {
            StreamState::Eof => Ok(Async::Ready(Inner::PlainText(Body::empty()))),
            StreamState::HasMore => Ok(Async::Ready(Inner::Gzip(Gzip::new(body))))
        }
    }
}

impl Gzip {
    fn new(stream: ReadableChunks<Body>) -> Self {
        Gzip {
            buf: BytesMut::with_capacity(INIT_BUFFER_SIZE),
            inner: Box::new(GzDecoder::new(stream)),
        }
    }
}

impl Stream for Gzip {
    type Item = Chunk;
    type Error = error::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.buf.remaining_mut() == 0 {
            self.buf.reserve(INIT_BUFFER_SIZE);
        }

        // The buffer contains uninitialised memory so getting a readable slice is unsafe.
        // We trust the `flate2` and `miniz` writer not to read from the memory given.
        //
        // To be safe, this memory could be zeroed before passing to `flate2`.
        // Otherwise we might need to deal with the case where `flate2` panics.
        let read = try_io!(self.inner.read(unsafe { self.buf.bytes_mut() }));

        if read == 0 {
            // If GzDecoder reports EOF, it doesn't necessarily mean the
            // underlying stream reached EOF (such as the `0\r\n\r\n`
            // header meaning a chunked transfer has completed). If it
            // isn't polled till EOF, the connection may not be able
            // to be re-used.
            //
            // See https://github.com/seanmonstar/reqwest/issues/508.
            let inner_read = try_io!(self.inner.get_mut().read(&mut [0]));
            if inner_read == 0 {
                Ok(Async::Ready(None))
            } else {
                Err(error::from(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected data after gzip decoder signaled end-of-file",
                )))
            }
        } else {
            unsafe { self.buf.advance_mut(read) };
            let chunk = Chunk::from_chunk(self.buf.split_to(read).freeze());

            Ok(Async::Ready(Some(chunk)))
        }
    }
}

/// A `Read`able wrapper over a stream of chunks.
pub struct ReadableChunks<S> {
    state: ReadState,
    stream: S,
}

enum ReadState {
    /// A chunk is ready to be read from.
    Ready(Chunk),
    /// The next chunk isn't ready yet.
    NotReady,
    /// The stream has finished.
    Eof,
}

enum StreamState {
    /// More bytes can be read from the stream.
    HasMore,
    /// No more bytes can be read from the stream.
    Eof
}

impl<S> ReadableChunks<S> {
    #[inline]
    pub(crate) fn new(stream: S) -> Self {
        ReadableChunks {
            state: ReadState::NotReady,
            stream: stream,
        }
    }
}

impl<S> fmt::Debug for ReadableChunks<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ReadableChunks")
            .finish()
    }
}

impl<S> Read for ReadableChunks<S>
where
    S: Stream<Item = Chunk, Error = error::Error>,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let ret;
            match self.state {
                ReadState::Ready(ref mut chunk) => {
                    let len = cmp::min(buf.len(), chunk.remaining());

                    buf[..len].copy_from_slice(&chunk[..len]);
                    chunk.advance(len);
                    if chunk.is_empty() {
                        ret = len;
                    } else {
                        return Ok(len);
                    }
                },
                ReadState::NotReady => {
                    match self.poll_stream() {
                        Ok(Async::Ready(StreamState::HasMore)) => continue,
                        Ok(Async::Ready(StreamState::Eof)) => {
                            return Ok(0)
                        },
                        Ok(Async::NotReady) => {
                            return Err(io::ErrorKind::WouldBlock.into())
                        },
                        Err(e) => {
                            return Err(error::into_io(e))
                        }
                    }
                },
                ReadState::Eof => return Ok(0),
            }
            self.state = ReadState::NotReady;
            return Ok(ret);
        }
    }
}

impl<S> ReadableChunks<S>
    where S: Stream<Item = Chunk, Error = error::Error>
{
    /// Poll the readiness of the inner reader.
    ///
    /// This function will update the internal state and return a simplified
    /// version of the `ReadState`.
    fn poll_stream(&mut self) -> Poll<StreamState, error::Error> {
        match self.stream.poll() {
            Ok(Async::Ready(Some(chunk))) => {
                self.state = ReadState::Ready(chunk);

                Ok(Async::Ready(StreamState::HasMore))
            },
            Ok(Async::Ready(None)) => {
                self.state = ReadState::Eof;

                Ok(Async::Ready(StreamState::Eof))
            },
            Ok(Async::NotReady) => {
                Ok(Async::NotReady)
            },
            Err(e) => Err(e)
        }
    }
}
