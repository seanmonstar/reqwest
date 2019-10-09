use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use futures::{Async, Future, Poll, Stream};
use futures::executor::{self, Notify};
use tokio_executor;

pub(crate) fn timeout<F>(fut: F, timeout: Option<Duration>) -> Result<F::Item, Waited<F::Error>>
where
    F: Future,
{
    let mut spawn = executor::spawn(fut);
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

static ENTER_WARNED: AtomicBool = AtomicBool::new(false);

fn block_on<F, U, E>(timeout: Option<Duration>, mut poll: F) -> Result<U, Waited<E>>
where
    F: FnMut(&Arc<ThreadNotify>) -> Poll<U, E>,
{
    // Check we're not already inside an `Executor`,
    // but for now, only log a warning if so.
    let _entered = tokio_executor::enter().map_err(|_| {
        if ENTER_WARNED.swap(true, Ordering::Relaxed) {
            trace!("warning: blocking API used inside an async Executor");
        } else {
            // If you're here wondering why you saw this message, it's because
            // something used `reqwest::Client`, which is a blocking
            // (synchronous) API, while within the context of an asynchronous
            // executor. For example, some async server.
            //
            // Doing this means that while you wait for the synchronous reqwest,
            // your server thread will be blocked, not being able to do
            // anything else.
            //
            // The best way to fix it is to use the `reqwest::async::Client`
            // instead, and return the futures for the async Executor to run
            // cooperatively.
            //
            // In future versions of reqwest, this will become an Error.
            //
            // For more: https://github.com/seanmonstar/reqwest/issues/541
            warn!("blocking API used inside an async Executor can negatively impact perfomance");
        }
    });
    let deadline = timeout.map(|d| {
        Instant::now() + d
    });
    let notify = Arc::new(ThreadNotify {
        thread: thread::current(),
    });

    loop {
        match poll(&notify)? {
            Async::Ready(val) => return Ok(val),
            Async::NotReady => {}
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


