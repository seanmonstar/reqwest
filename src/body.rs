use std::io::Read;
use std::fs::File;
use std::fmt;

/// Body type for a request.
#[derive(Debug)]
pub struct Body {
    reader: Kind,
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
    /// A `Body` constructed from a set of bytes, like `String` or `Vec<u8>`,
    /// are stored differently and can be reused.
    ///
    /// ```rust
    /// # use reqwest::Body;
    /// # use std::fs::File;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// // std::fs::File implements std::io::Read
    /// let file = File::open("national_secrets.txt")?;
    /// let body = Body::new(file);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new<R: Read + Send + 'static>(reader: R) -> Body {
        Body {
            reader: Kind::Reader(Box::new(reader), None),
        }
    }

    /// Create a `Body` from a `Reader` where we can predict the size in
    /// advance, but where we don't want to load the data in memory.  This
    /// is useful if we need to ensure `Content-Length` is passed with the
    /// request.
    ///
    /// ```rust
    /// # use reqwest::Body;
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// // &[u8] implements std::io::Read, and the source `s` has a
    /// // 'static lifetime and a known number of bytes.
    /// let s = "A predictable body";
    /// let bytes = s.as_bytes();
    /// let size  = bytes.len() as u64;
    /// let body = Body::sized(bytes, size);
    /// # Ok(())
    /// # }
    /// ```
    pub fn sized<R: Read + Send + 'static>(reader: R, len: u64) -> Body {
        Body {
            reader: Kind::Reader(Box::new(reader), Some(len)),
        }
    }

    /*
    pub fn chunked(reader: ()) -> Body {
        unimplemented!()
    }
    */
}

// useful for tests, but not publicly exposed
#[cfg(test)]
pub fn read_to_string(mut body: Body) -> ::std::io::Result<String> {
    let mut s = String::new();
    match body.reader {
            Kind::Reader(ref mut reader, _) => reader.read_to_string(&mut s),
            Kind::Bytes(ref mut bytes) => (&**bytes).read_to_string(&mut s),
        }
        .map(|_| s)
}

enum Kind {
    Reader(Box<Read + Send>, Option<u64>),
    Bytes(Vec<u8>),
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
        match *self {
            Kind::Reader(_, ref v) => f.debug_tuple("Kind::Reader").field(&"_").field(v).finish(),
            Kind::Bytes(ref v) => f.debug_tuple("Kind::Bytes").field(v).finish(),
        }
    }
}


// Wraps a `std::io::Write`.
//pub struct Pipe(Kind);


pub fn as_hyper_body(body: &mut Body) -> ::hyper::client::Body {
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

pub fn can_reset(body: &Body) -> bool {
    match body.reader {
        Kind::Bytes(_) => true,
        Kind::Reader(..) => false,
    }
}
