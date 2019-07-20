use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::{Poll, Stream, TryFuture};
use futures::executor::{self, Notify};
use tokio_executor::{enter, EnterError};

pub(crate) fn timeout<F>(fut: F, timeout: Option<Duration>) -> Result<F::Ok, Waited<F::Error>>
where
    F: TryFuture,
{
    let mut spawn = executor::spawn(Box::new(fut));
    block_on(timeout, |notify| {
        spawn.poll_future_notify(notify, 0)
    })
}

pub(crate) fn stream<S>(stream: S, timeout: Option<Duration>) -> WaitStream<S>
where S: Stream {
    WaitStream {
        stream: executor::spawn(stream),
        timeout: timeout,
    }
}

#[derive(Debug)]
pub(crate) enum Waited<E> {
    TimedOut,
    Executor(EnterError),
    Inner(E),
}

impl<E> From<E> for Waited<E> {
    fn from(err: E) -> Waited<E> {
        Waited::Inner(err)
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
    F: FnMut(&Arc<ThreadNotify>) -> Poll<Result<Result<U, E>>>,
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
