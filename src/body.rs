use std::io::Read;

/// Body type for a request.
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

enum Kind {
    Reader(Box<Read>, Option<u64>),
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



// Wraps a `std::io::Write`.
//pub struct Pipe(Kind);


pub fn as_hyper_body<'a>(body: &'a Body) -> ::hyper::client::Body<'a> {
    match body.reader {
        Kind::Bytes(ref bytes) => {
            let len = bytes.len();
            ::hyper::client::Body::BufBody(bytes, len)
        },
        Kind::Reader(..) => unimplemented!()
    }
}

