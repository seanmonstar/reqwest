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
- `Peeked` is a buffer that keeps a few bytes available so `libflate`s `read_exact` calls won't fail
*/

use std::fmt;
use std::mem;
use std::cmp;
use std::io::{self, Read};
use std::marker::PhantomData;

use bytes::{BufMut, BytesMut};
use libflate::non_blocking::gzip;
use tokio_io::AsyncRead;
use tokio_io::io as async_io;
use futures::{Async, Future, Poll, Stream};
use futures::stream::Concat2;
use hyper::StatusCode;
use serde::de::DeserializeOwned;
use serde_json;
use url::Url;

use header::{Headers, ContentEncoding, ContentLength, Encoding, TransferEncoding};
use super::{body, Body, Chunk};
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

enum Pending {
    /// An unreachable internal state.
    Empty,
    /// A future attempt to poll the response body for EOF so we know whether to use gzip or not.
    Gzip(ReadableChunks<Body>)
}

/// A gzip decoder that reads from a `libflate::gzip::Decoder` into a `BytesMut` and emits the results
/// as a `Chunk`.
struct Gzip {
    inner: gzip::Decoder<Peeked<ReadableChunks<Body>>>,
    buf: BytesMut,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Decoder")
            .finish()
    }
}

impl Decoder {
    #[inline]
    fn plain_text(body: Body) -> Decoder {
        Decoder {
            inner: Inner::PlainText(body)
        }
    }

    #[inline]
    fn gzip(mut body: Body) -> Decoder {
        Decoder {
            inner: Inner::Pending(Pending::Gzip(ReadableChunks::new(body)))
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
        let body_state = match *self {
            Pending::Gzip(ref mut body) => {
                match body.poll_stream() {
                    Ok(Async::Ready(state)) => state,
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(e) => return Err(e)
                }
            },
            Pending::Empty => panic!("poll for a decoder after it's done")
        };

        match mem::replace(self, Pending::Empty) {
            Pending::Gzip(body) => {
                // libflate does a read_exact([0; 2]), so its impossible to tell
                // if the stream was empty, or truly had an UnexpectedEof.
                // Therefore, we need to check for EOF first.
                match body_state {
                    StreamState::Eof => Ok(Async::Ready(Inner::PlainText(body::empty()))),
                    StreamState::HasMore => Ok(Async::Ready(Inner::Gzip(Gzip::new(body))))
                }
            },
            Pending::Empty => panic!("invalid internal state")
        }
    }
}

impl Gzip {
    fn new(stream: ReadableChunks<Body>) -> Self {
        Gzip {
            buf: BytesMut::with_capacity(INIT_BUFFER_SIZE),
            inner: gzip::Decoder::new(Peeked::new(stream))
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
        // We trust the `libflate` writer not to read from the memory given.
        // 
        // To be safe, this memory could be zeroed before passing to `libflate`.
        // Otherwise we might need to deal with the case where `libflate` panics.
        let read = {
            let mut buf = unsafe { self.buf.bytes_mut() };
            self.inner.read(&mut buf)
        };

        match read {
            Ok(read) if read == 0 => {
                Ok(Async::Ready(None))
            },
            Ok(read) => {
                unsafe { self.buf.advance_mut(read) };
                let chunk = body::chunk(self.buf.split_to(read).freeze());

                Ok(Async::Ready(Some(chunk)))
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                Ok(Async::NotReady)
            },
            Err(e) => Err(error::from(e))
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
    Ready(Chunk, usize),
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

/// A buffering reader that ensures `Read`s return at least a few bytes.
struct Peeked<R> {
    state: PeekedState,
    peeked_buf: [u8; 2],
    pos: usize,
    inner: R,
}

enum PeekedState {
    /// The internal buffer hasn't filled yet.
    NotReady,
    /// The internal buffer can be read.
    Ready(usize)
}

impl<R> Peeked<R> {
    #[inline]
    fn new(inner: R) -> Self {
        Peeked {
            state: PeekedState::NotReady,
            peeked_buf: [0; 2],
            inner: inner,
            pos: 0,
        }
    }

    #[inline]
    fn ready(&mut self) {
        self.state = PeekedState::Ready(self.pos);
        self.pos = 0;
    }

    #[inline]
    fn not_ready(&mut self) {
        self.state = PeekedState::NotReady;
        self.pos = 0;
    }
}

impl<R: Read> Read for Peeked<R> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.state {
                PeekedState::Ready(peeked_buf_len) => {
                    let len = cmp::min(buf.len(), peeked_buf_len - self.pos);
                    let start = self.pos;
                    let end = self.pos + len;

                    buf[..len].copy_from_slice(&self.peeked_buf[start..end]);
                    self.pos += len;
                    if self.pos == peeked_buf_len {
                        self.not_ready();
                    }

                    return Ok(len)
                },
                PeekedState::NotReady => {
                    let read = self.inner.read(&mut self.peeked_buf[self.pos..]);

                    match read {
                        Ok(0) => {
                            self.ready();
                        },
                        Ok(read) => {
                            self.pos += read;
                            if self.pos == self.peeked_buf.len() {
                                self.ready();
                            }
                        },
                        Err(e) => return Err(e)
                    }
                }
            };
        }
    }
}

impl<S> ReadableChunks<S> {
    #[inline]
    pub fn new(stream: S) -> Self {
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
    where S: Stream<Item = Chunk, Error = error::Error>
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let ret;
            match self.state {
                ReadState::Ready(ref mut chunk, ref mut pos) => {
                    let chunk_start = *pos;
                    let len = cmp::min(buf.len(), chunk.len() - chunk_start);
                    let chunk_end = chunk_start + len;

                    buf[..len].copy_from_slice(&chunk[chunk_start..chunk_end]);
                    *pos += len;
                    if *pos == chunk.len() {
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
                self.state = ReadState::Ready(chunk, 0);

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

// pub(crate)

#[inline]
pub fn take(decoder: &mut Decoder) -> Decoder {
    let inner = mem::replace(&mut decoder.inner, Inner::PlainText(body::empty()));
    Decoder {
        inner: inner,
    }
}

/// Constructs a Decoder from a hyper request.
///
/// A decoder is just a wrapper around the hyper request that knows
/// how to decode the content body of the request.
///
/// Uses the correct variant by inspecting the Content-Encoding header.
pub fn detect(headers: &mut Headers, body: Body, check_gzip: bool) -> Decoder {
    if !check_gzip {
        return Decoder::plain_text(body);
    }
    let content_encoding_gzip: bool;
    let mut is_gzip = {
        content_encoding_gzip = headers
            .get::<ContentEncoding>()
            .map_or(false, |encs| encs.contains(&Encoding::Gzip));
        content_encoding_gzip ||
        headers
            .get::<TransferEncoding>()
            .map_or(false, |encs| encs.contains(&Encoding::Gzip))
    };
    if is_gzip {
        if let Some(content_length) = headers.get::<ContentLength>() {
            if content_length.0 == 0 {
                warn!("GZipped response with content-length of 0");
                is_gzip = false;
            }
        }
    }
    if content_encoding_gzip {
        headers.remove::<ContentEncoding>();
        headers.remove::<ContentLength>();
    }
    if is_gzip {
        Decoder::gzip(body)
    } else {
        Decoder::plain_text(body)
    }
}
