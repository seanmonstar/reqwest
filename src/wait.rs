use std::time::{Duration, Instant};
use std::pin::Pin;
use std::task::{Context, Poll};
use futures::future::TryFutureExt;
use tokio::future::FutureExt;

use futures::{Future, Stream};
use tokio::timer::Delay;

async fn timeout_fut<F, I, E>(fut: F, timeout: Option<Duration>) -> Result<I, Waited<E>>
    where F: Future<Output = Result<I,E>> {

    let result: Result<I, Waited<E>> = if let Some(duration) = timeout {
        match fut.map_err(|e| Waited::Inner(e)).timeout(duration).await {
            Err(_e) => Err(Waited::TimedOut),
            Ok(r) => r,
        }
    } else {
        fut.map_err(|e| Waited::Inner(e)).await
    };

    result
}

pub(crate) fn timeout<F, I, E>(fut: F, timeout : Option<Duration>) -> Result<I, Waited<E>>
    where F: Future<Output = Result<I,E>> {
        let f = timeout_fut::<F, I, E>(fut, timeout);

        match tokio::runtime::Runtime::new() {
            Err(e) => Err(Waited::Executor(e)),
            Ok(rt) => rt.block_on(f),
        }
    }

//TODO: Needed?
//pub(crate) fn stream<S>(stream: S, timeout: Option<Duration>) -> WaitStream<S>
//where S: Stream {
//    WaitStream {
//        inner: stream,
//        pending: None,
//        timeout: timeout,
//    }
//}

#[derive(Debug)]
pub(crate) enum Waited<E> {
    TimedOut,
    Executor(std::io::Error),
    Inner(E),
}

pub(crate) struct WaitStream<S> {
    inner: S,
    pending: Option<Delay>,
    timeout: Option<Duration>,
}

impl<S> WaitStream<S> {
    fn inner(self: Pin<&mut Self>) -> Pin<&mut S> {
        unsafe {
            Pin::map_unchecked_mut(self, |x| &mut x.inner)
        }
    }
    fn pending(self: Pin<&mut Self>) -> Pin<&mut Option<Delay>> {
        unsafe {
            Pin::map_unchecked_mut(self, |x| &mut x.pending)
        }
    }
}

impl<S, T, E> Stream for WaitStream<S> where S: Stream<Item=Result<T, E>> {
    type Item = Result<T, Waited<E>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Poll::Ready(v) = self.as_mut().inner().as_mut().poll_next(cx) {
            let r = match v {
                None => Poll::Ready(None),
                Some(Err(e)) => Poll::Ready(Some(Err(Waited::Inner(e)))),
                Some(Ok(v)) => Poll::Ready(Some(Ok(v))),
            };
            return r;
        }

        if let Some(to) = self.timeout.clone() {
            match self.as_mut().pending().as_pin_mut() {
                None => {
                    self.as_mut().pending().get_mut().replace(tokio::timer::Delay::new(Instant::now() + to));
                    Poll::Pending
                }
                Some(pending) => {
                    let r = match pending.poll(cx) {
                        Poll::Pending => Poll::Pending,
                        Poll::Ready(_) => Poll::Ready(Some(Err(Waited::TimedOut))),
                    };
                    r
                }
            }
        } else {
            Poll::Pending
        }
    }
}