use std::io::{Cursor, Read};
use std::io;
use std::fs::File;
use std::fmt;

/// Body type for a request.
#[derive(Debug)]
pub struct Body {
    reader: Kind,
}

impl Body {
    /// Instantiate a `Body` from a reader.
    pub fn new<R: Read + 'static>(reader: R) -> Body {
        Body {
            reader: Kind::Reader(Box::new(reader), None),
        }
    }

    /*
    pub fn sized(reader: (), len: u64) -> Body {
        unimplemented!()
    }

    pub fn chunked(reader: ()) -> Body {
        unimplemented!()
    }
    */
}

impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

enum Kind {
    Reader(Box<Read>, Option<u64>),
    Bytes(Vec<u8>),
}

impl Read for Kind {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            Kind::Reader(ref mut reader, _) => {
                reader.read(buf)
            }
            Kind::Bytes(ref mut bytes) => {
                // To make sure the bytes are removed properly when you read
                // them, you need to use `drain()`
                // FIXME: this will probably have poor performance for larger
                // bodies due to allocating more than necessary.
                let drained_bytes: Vec<u8> = bytes.drain(..).collect();

                // then you need a Cursor because a standard Vec doesn't implement
                // Read
                let mut cursor = Cursor::new(drained_bytes);
                cursor.read(buf)
            }
        }
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(v: Vec<u8>) -> Body {
        Body {
            reader: Kind::Bytes(v),
        }
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        s.into_bytes().into()
    }
}


impl<'a> From<&'a [u8]> for Body {
    #[inline]
    fn from(s: &'a [u8]) -> Body {
        s.to_vec().into()
    }
}

impl<'a> From<&'a str> for Body {
    #[inline]
    fn from(s: &'a str) -> Body {
        s.as_bytes().into()
    }
}

impl From<File> for Body {
    #[inline]
    fn from(f: File) -> Body {
        let len = f.metadata().map(|m| m.len()).ok();
        Body {
            reader: Kind::Reader(Box::new(f), len),
        }
    }
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &Kind::Reader(_, ref v) => f.debug_tuple("Kind::Reader").field(&"_").field(v).finish(),
            &Kind::Bytes(ref v) => f.debug_tuple("Kind::Bytes").field(v).finish(),
        }
    }
}


// Wraps a `std::io::Write`.
//pub struct Pipe(Kind);


pub fn as_hyper_body<'a>(body: &'a mut Body) -> ::hyper::client::Body<'a> {
    match body.reader {
        Kind::Bytes(ref bytes) => {
            let len = bytes.len();
            ::hyper::client::Body::BufBody(bytes, len)
        }
        Kind::Reader(ref mut reader, len_opt) => {
            match len_opt {
                Some(len) => ::hyper::client::Body::SizedBody(reader, len),
                None => ::hyper::client::Body::ChunkedBody(reader),
            }
        }
    }
}
