use std::fmt;
use bytes::{Buf, Bytes};
use hyper::body::Payload;
use tokio::timer::Delay;
use std::pin::Pin;
use std::future::Future;
use futures::Stream;
use std::task::{Poll, Context};

/// An asynchronous `Stream`.
pub struct Body {
    inner: Inner,
}

enum Inner {
    Reusable(Bytes),
    Hyper {
        body: hyper::Body,
        timeout: Option<Delay>,
    }
}

impl Body {
    pub(crate) fn content_length(&self) -> Option<u64> {
        match self.inner {
            Inner::Reusable(ref bytes) => Some(bytes.len() as u64),
            Inner::Hyper { ref body, .. } => body.content_length(),
        }
    }

    #[inline]
    pub(crate) fn response(body: hyper::Body, timeout: Option<Delay>) -> Body {
        Body {
            inner: Inner::Hyper {
                body,
                timeout,
            },
        }
    }

    #[inline]
    pub(crate) fn wrap(body: hyper::Body) -> Body {
        Body {
            inner: Inner::Hyper {
                body,
                timeout: None,
            },
        }
    }

    #[inline]
    pub(crate) fn empty() -> Body {
        Body::wrap(hyper::Body::empty())
    }

    #[inline]
    pub(crate) fn reusable(chunk: Bytes) -> Body {
        Body {
            inner: Inner::Reusable(chunk),
        }
    }

    #[inline]
    pub(crate) fn into_hyper(self) -> (Option<Bytes>, hyper::Body) {
        match self.inner {
            Inner::Reusable(chunk) => (Some(chunk.clone()), chunk.into()),
            Inner::Hyper { body, timeout } => {
                debug_assert!(timeout.is_none());
                (None, body)
            },
        }
    }

    fn inner(self: Pin<&mut Self>) -> Pin<&mut Inner> {
        unsafe {
            Pin::map_unchecked_mut(self, |x| &mut x.inner)
        }
    }
}

impl Stream for Body {
    type Item = Result<Chunk, crate::Error>;

    #[inline]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let opt_try_chunk = match self.inner().get_mut() {
            Inner::Hyper { ref mut body, ref mut timeout } => {
                if let Some(ref mut timeout) = timeout {
                    if let Poll::Ready(()) = Pin::new(timeout).poll(cx) {
                        return Poll::Ready(Some(Err(crate::error::timedout(None))));
                    }
                }
                futures::ready!(Pin::new(body).poll_data(cx))
                    .map(|opt_chunk| {
                        opt_chunk.map(|c| Chunk { inner: c })
                            .map_err(crate::error::from)
                    })
            },
            Inner::Reusable(ref mut bytes) => {
                if bytes.is_empty() {
                    None
                } else {
                    let chunk = Chunk::from_chunk(bytes.clone());
                    *bytes = Bytes::new();
                    Some(Ok(chunk))
                }
            },
        };

        Poll::Ready(opt_try_chunk)
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(bytes: Bytes) -> Body {
        Body::reusable(bytes)
    }
}

impl From<Vec<u8>> for Body {
    #[inline]
    fn from(vec: Vec<u8>) -> Body {
        Body::reusable(vec.into())
    }
}

impl From<&'static [u8]> for Body {
    #[inline]
    fn from(s: &'static [u8]) -> Body {
        Body::reusable(Bytes::from_static(s))
    }
}

impl From<String> for Body {
    #[inline]
    fn from(s: String) -> Body {
        Body::reusable(s.into())
    }
}

impl From<&'static str> for Body {
    #[inline]
    fn from(s: &'static str) -> Body {
        s.as_bytes().into()
    }
}

impl<I, E> From<Pin<Box<dyn Stream<Item = Result<I, E>> + Send + Sync>>> for Body
where
    hyper::Chunk: From<I>,
    I: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    #[inline]
    fn from(s: Box<dyn Stream<Item = I, Error = E> + Send>) -> Body {
        Body::wrap(hyper::Body::wrap_stream(s))
    }
}

/// A chunk of bytes for a `Body`.
///
/// A `Chunk` can be treated like `&[u8]`.
#[derive(Default)]
pub struct Chunk {
    inner: hyper::Chunk,
}

impl Chunk {
    #[inline]
    pub(crate) fn from_chunk(chunk: Bytes) -> Chunk {
        Chunk {
            inner: hyper::Chunk::from(chunk)
        }
    }
}
impl Buf for Chunk {
    fn bytes(&self) -> &[u8] {
        self.inner.bytes()
    }

    fn remaining(&self) -> usize {
        self.inner.remaining()
    }

    fn advance(&mut self, n: usize) {
        self.inner.advance(n);
    }
}

impl AsRef<[u8]> for Chunk {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &*self
    }
}

impl std::ops::Deref for Chunk {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

impl Extend<u8> for Chunk {
    fn extend<T>(&mut self, iter: T)
    where T: IntoIterator<Item=u8> {
        self.inner.extend(iter)
    }
}

impl IntoIterator for Chunk {
    type Item = u8;
    //XXX: exposing type from hyper!
    type IntoIter = <hyper::Chunk as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl From<Chunk> for hyper::Chunk {
  fn from(val: Chunk) -> hyper::Chunk {
    val.inner
  }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Body")
            .finish()
    }
}

impl fmt::Debug for Chunk {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}
