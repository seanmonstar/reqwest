use bytes::Bytes;
use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, Read};

use super::wasi::http::*;
use super::wasi::io::*;

use super::client::failure_point;

/// An asynchronous request body.
#[derive(Debug)]
pub struct Body {
    kind: Option<Kind>,
}

impl Body {
    /// Instantiate a `Body` from a reader.
    ///
    /// # Note
    ///
    /// While allowing for many types to be used, these bodies do not have
    /// a way to reset to the beginning and be reused. This means that when
    /// encountering a 307 or 308 status code, instead of repeating the
    /// request at the new location, the `Response` will be returned with
    /// the redirect status code set.
    ///
    /// ```rust
    /// # use std::fs::File;
    /// # use reqwest::blocking::Body;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let file = File::open("national_secrets.txt")?;
    /// let body = Body::new(file);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// If you have a set of bytes, like `String` or `Vec<u8>`, using the
    /// `From` implementations for `Body` will store the data in a manner
    /// it can be reused.
    ///
    /// ```rust
    /// # use reqwest::blocking::Body;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let s = "A stringy body";
    /// let body = Body::from(s);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new<R: Read + Send + 'static>(reader: R) -> Body {
        Body {
            kind: Some(Kind::Reader(Box::from(reader), None)),
        }
    }

    /// Create a `Body` from a `Read` where the size is known in advance
    /// but the data should not be fully loaded into memory. This will
    /// set the `Content-Length` header and stream from the `Read`.
    ///
    /// ```rust
    /// # use std::fs::File;
    /// # use reqwest::blocking::Body;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let file = File::open("a_large_file.txt")?;
    /// let file_size = file.metadata()?.len();
    /// let body = Body::sized(file, file_size);
    /// # Ok(())
    /// # }
    /// ```
    pub fn sized<R: Read + Send + 'static>(reader: R, len: u64) -> Body {
        Body {
            kind: Some(Kind::Reader(Box::from(reader), Some(len))),
        }
    }

    /// Returns the body as a byte slice if the body is already buffered in
    /// memory. For streamed requests this method returns `None`.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self.kind {
            Some(Kind::Reader(_, _)) => None,
            Some(Kind::Bytes(ref bytes)) => Some(bytes.as_ref()),
            Some(Kind::Incoming(_)) => None,
            None => None
        }
    }

    /// Converts streamed requests to their buffered equivalent and
    /// returns a reference to the buffer. If the request is already
    /// buffered, this has no effect.
    ///
    /// Be aware that for large requests this method is expensive
    /// and may cause your program to run out of memory.
    pub fn buffer(&mut self) -> Result<&[u8], crate::Error> {
        match self.kind {
            Some(Kind::Reader(ref mut reader, maybe_len)) => {
                let mut bytes = if let Some(len) = maybe_len {
                    Vec::with_capacity(len as usize)
                } else {
                    Vec::new()
                };
                io::copy(reader, &mut bytes).map_err(crate::error::builder)?;
                self.kind = Some(Kind::Bytes(bytes.into()));
                self.buffer()
            }
            Some(Kind::Bytes(ref bytes)) => Ok(bytes.as_ref()),
            Some(Kind::Incoming(handle)) => {
                let mut bytes = Vec::new();
                let mut eof = false;
                while !eof {
                    let (mut body_chunk, stream_status) = streams::read(handle, u64::MAX).map_err(|e| failure_point("read", e))?;
                    eof = stream_status == streams::StreamStatus::Ended;
                    bytes.append(&mut body_chunk);
                }
                self.kind = Some(Kind::Bytes(bytes.into()));
                self.buffer()
            }
            None => panic!("Body has already been extracted")
        }
    }

    pub(crate) fn into_reader(mut self) -> Reader {
        match self.kind.take() {
            Some(Kind::Reader(r, _)) => Reader::Reader(r),
            Some(Kind::Bytes(b)) => Reader::Bytes(Cursor::new(b)),
            Some(Kind::Incoming(handle)) => Reader::Wasi(handle),
            None => panic!("Body has already been extracted")
        }
    }

    pub(crate) fn try_clone(&self) -> Option<Body> {
        self.kind.as_ref().unwrap().try_clone().map(|kind| Body { kind: Some(kind) })
    }

    pub(crate) fn write(mut self, mut f: impl FnMut(&[u8]) -> Result<(), crate::Error>) -> Result<(), crate::Error> {
        match self.kind.take().expect("Body has already been extracted") {
            Kind::Reader(mut reader, _) => {
                let mut buf = [0; 8 * 1024];
                loop {
                    let len = reader.read(&mut buf).map_err(crate::error::builder)?;
                    if len == 0 {
                        break;
                    }
                    f(&buf[..len])?;
                }
                Ok(())
            }
            Kind::Bytes(bytes) => f(&bytes),
            Kind::Incoming(handle) => {
                let mut eof = false;
                while !eof {
                    let (body_chunk, stream_status) = streams::read(handle, u64::MAX).map_err(|e| failure_point("read", e))?;
                    eof = stream_status == streams::StreamStatus::Ended;
                    f(&body_chunk)?;
                }
                Ok(())
            }
        }
    }
}

enum Kind {
    Reader(Box<dyn Read + Send>, Option<u64>),
    Bytes(Bytes),
    Incoming(types::IncomingStream),
}

impl Kind {
    fn try_clone(&self) -> Option<Kind> {
        match self {
            Kind::Reader(..) => None,
            Kind::Bytes(v) => Some(Kind::Bytes(v.clone())),
            Kind::Incoming(..) => None
        }
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(v: Vec<u8>) -> Body {
        Body {
            kind: Some(Kind::Bytes(v.into())),
        }
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        s.into_bytes().into()
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Body {
        Body {
            kind: Some(Kind::Bytes(Bytes::from_static(s))),
        }
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Body {
        s.as_bytes().into()
    }
}

impl From<File> for Body {
    #[inline]
    fn from(f: File) -> Body {
        let len = f.metadata().map(|m| m.len()).ok();
        Body {
            kind: Some(Kind::Reader(Box::new(f), len)),
        }
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(b: Bytes) -> Body {
        Body {
            kind: Some(Kind::Bytes(b)),
        }
    }
}

impl From<types::IncomingStream> for Body {
    #[inline]
    fn from(s: types::IncomingStream) -> Body {
        Body {
            kind: Some(Kind::Incoming(s)),
        }
    }
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Kind::Reader(_, ref v) => f
                .debug_struct("Reader")
                .field("length", &DebugLength(v))
                .finish(),
            Kind::Bytes(ref v) => fmt::Debug::fmt(v, f),
            Kind::Incoming(_) => f.debug_struct("Incoming").finish()
        }
    }
}

struct DebugLength<'a>(&'a Option<u64>);

impl<'a> fmt::Debug for DebugLength<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self.0 {
            Some(ref len) => fmt::Debug::fmt(len, f),
            None => f.write_str("Unknown"),
        }
    }
}

pub(crate) enum Reader {
    Reader(Box<dyn Read + Send>),
    Bytes(Cursor<Bytes>),
    Wasi(types::IncomingStream),
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            Reader::Reader(ref mut rdr) => rdr.read(buf),
            Reader::Bytes(ref mut rdr) => rdr.read(buf),
            Reader::Wasi(handle) => {
                let (body_chunk, stream_status) = streams::read(handle, buf.len() as u64).map_err(|_| io::Error::new(io::ErrorKind::Other, "read chunk"))?;
                if stream_status == streams::StreamStatus::Ended {
                    return Ok(0);
                } else {
                    let len = body_chunk.len();
                    buf[..len].copy_from_slice(&body_chunk);
                    Ok(len)
                }
            }
        }
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        match self.kind {
            Some(Kind::Incoming(handle)) => streams::drop_input_stream(handle),
            _ => {}
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        match self {
            Reader::Wasi(handle) => streams::drop_input_stream(*handle),
            _ => {}
        }
    }
}