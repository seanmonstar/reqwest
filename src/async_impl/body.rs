use bytes::Bytes;
use futures_core::Stream;
use hyper::body::Payload;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::timer::Delay;

/// An asynchronous request body.
pub struct Body {
    inner: Inner,
}

// The `Stream` trait isn't stable, so the impl isn't public.
pub(crate) struct ImplStream(Body);

enum Inner {
    Reusable(Bytes),
    Hyper {
        body: hyper::Body,
        timeout: Option<Delay>,
    },
}

impl Body {
    /// Wrap a futures `Stream` in a box inside `Body`.
    ///
    /// # Example
    ///
    /// ```
    /// # use reqwest::Body;
    /// # use futures_util;
    /// # fn main() {
    /// let chunks: Vec<Result<_, ::std::io::Error>> = vec![
    ///     Ok("hello"),
    ///     Ok(" "),
    ///     Ok("world"),
    /// ];
    ///
    /// let stream = futures_util::stream::iter(chunks);
    ///
    /// let body = Body::wrap_stream(stream);
    /// # }
    /// ```
    pub fn wrap_stream<S>(stream: S) -> Body
    where
        S: futures_core::stream::TryStream + Send + Sync + 'static,
        S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        hyper::Chunk: From<S::Ok>,
    {
        Body::wrap(hyper::body::Body::wrap_stream(stream))
    }

    pub(crate) fn response(body: hyper::Body, timeout: Option<Delay>) -> Body {
        Body {
            inner: Inner::Hyper { body, timeout },
        }
    }

    pub(crate) fn wrap(body: hyper::Body) -> Body {
        Body {
            inner: Inner::Hyper {
                body,
                timeout: None,
            },
        }
    }

    pub(crate) fn empty() -> Body {
        Body::wrap(hyper::Body::empty())
    }

    pub(crate) fn reusable(chunk: Bytes) -> Body {
        Body {
            inner: Inner::Reusable(chunk),
        }
    }

    pub(crate) fn into_hyper(self) -> (Option<Bytes>, hyper::Body) {
        match self.inner {
            Inner::Reusable(chunk) => (Some(chunk.clone()), chunk.into()),
            Inner::Hyper { body, timeout } => {
                debug_assert!(timeout.is_none());
                (None, body)
            }
        }
    }

    pub(crate) fn into_stream(self) -> ImplStream {
        ImplStream(self)
    }

    pub(crate) fn content_length(&self) -> Option<u64> {
        match self.inner {
            Inner::Reusable(ref bytes) => Some(bytes.len() as u64),
            Inner::Hyper { ref body, .. } => body.size_hint().exact(),
        }
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

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Body").finish()
    }
}

// ===== impl ImplStream =====

impl Stream for ImplStream {
    type Item = Result<Bytes, crate::Error>;

    #[inline]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let opt_try_chunk = match self.0.inner {
            Inner::Hyper {
                ref mut body,
                ref mut timeout,
            } => {
                if let Some(ref mut timeout) = timeout {
                    if let Poll::Ready(()) = Pin::new(timeout).poll(cx) {
                        return Poll::Ready(Some(Err(crate::error::timedout(None))));
                    }
                }
                futures_core::ready!(Pin::new(body).poll_data(cx))
                    .map(|opt_chunk| opt_chunk.map(Into::into).map_err(crate::error::from))
            }
            Inner::Reusable(ref mut bytes) => {
                if bytes.is_empty() {
                    None
                } else {
                    Some(Ok(std::mem::replace(bytes, Bytes::new())))
                }
            }
        };

        Poll::Ready(opt_try_chunk)
    }
}
