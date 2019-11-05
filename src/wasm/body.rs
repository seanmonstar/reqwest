/// dox
use bytes::Bytes;
use std::fmt;

/// The body of a `Request`.
///
/// In most cases, this is not needed directly, as the
/// [`RequestBuilder.body`][builder] method uses `Into<Body>`, which allows
/// passing many things (like a string or vector of bytes).
///
/// [builder]: ./struct.RequestBuilder.html#method.body
pub struct Body(Bytes);

impl Body {
    pub(crate) fn bytes(&self) -> &Bytes {
        &self.0
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(bytes: Bytes) -> Body {
        Body(bytes)
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(vec: Vec<u8>) -> Body {
        Body(vec.into())
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Body {
        Body(Bytes::from_static(s))
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        Body(s.into())
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Body {
        s.as_bytes().into()
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Body").finish()
    }
}
