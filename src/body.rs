use std::io::Read;

pub struct Body(Kind);

impl Body {
    pub fn sized(reader: (), len: u64) -> Body {
        unimplemented!()
    }

    pub fn chunked(reader: ()) -> Body {
        unimplemented!()
    }
}

enum Kind {
    Length,
    Chunked
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(v: Vec<u8>) -> Body {
        unimplemented!()
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        s.into_bytes().into()
    }
}

/// Wraps a `std::io::Write`.
pub struct Pipe(Kind);


