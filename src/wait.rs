use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use futures_timer::TryFutureExt as TryFutureTimerExt;
use futures::future::TryFutureExt;

use futures::{Future, Poll, Stream};
use std::pin::Pin;
use std::task::Context;

async fn timeout_fut<F, I, E>(fut: F, timeout: Option<Duration>) -> Result<I, Waited<E>>
    where F: Future<Output = Result<I,E>> {

    let result: Result<I, Waited<E>> = if let Some(duration) = timeout {
        fut.map_err(|e| Waited::Inner(e)).timeout(duration).await
    } else {
        fut.map_err(|e| Waited::Inner(e)).await
    };

    result
}

pub(crate) fn timeout<F, I, E>(fut: F, timeout : Option<Duration>) -> Result<I, Waited<E>>
    where F: Future<Output = Result<I,E>> {
        let future03 = timeout_fut::<F, I, E>();

        let future01 = future03.compat();

        tokio::runtime::Runtime::new()
        .map_err(|e| Waited::Executor(e))
        .map(|runtime| runtime.block_on(future01))
    }

pub(crate) fn stream<S>(stream: S, timeout: Option<Duration>) -> WaitStream<S>
where S: Stream {
    WaitStream {
        stream: executor::spawn(stream),
        timeout,
    }
}

#[derive(Debug)]
pub(crate) enum Waited<E> {
    TimedOut,
    Inner(E),
    Executor(E),
}

impl<E> From<std::io::Error> for Waited<E> {
    fn from(err: std::io::Error) -> Waited<E> {
        Waited::Executor
    }
}

pub(crate) struct WaitFuture<F> 
where F: Future
{
    future: F,
    timeout: Option<Duration>,
}

impl<F, I, E> Future for WaitFuture<F>
where F: Future<Output = Result<I,E>>
{
    type Output = Result<I, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        
    }
}

pub(crate) struct WaitStream<S> {
    stream: executor::Spawn<S>,
    timeout: Option<Duration>,
}

impl<S> Iterator for WaitStream<S>
where S: Stream {
    type Item = Result<S::Item, Waited<S::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        let res = block_on(self.timeout, |notify| {
            self.stream.poll_stream_notify(notify, 0)
        });

        match res {
            Ok(Some(val)) => Some(Ok(val)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
    }
}

struct ThreadNotify {
    thread: thread::Thread,
}

impl Notify for ThreadNotify {
    fn notify(&self, _id: usize) {
        self.thread.unpark();
    }
}

fn block_on<F, U, E>(timeout: Option<Duration>, mut poll: F) -> Result<U, Waited<E>>
where
    F: FnMut(&Arc<ThreadNotify>) -> Poll<U, E>,
{
    let _entered = enter().map_err(Waited::Executor)?;
    let deadline = timeout.map(|d| {
        Instant::now() + d
    });
    let notify = Arc::new(ThreadNotify {
        thread: thread::current(),
    });

    loop {
        match poll(&notify)? {
            Poll::Ready(val) => return Ok(val),
            Poll::Pending => {}
        }

        if let Some(deadline) = deadline {
            let now = Instant::now();
            if now >= deadline {
                return Err(Waited::TimedOut);
            }

            thread::park_timeout(deadline - now);
        } else {
            thread::park();
        }
    }
}


