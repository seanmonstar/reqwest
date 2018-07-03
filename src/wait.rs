use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::{Async, Future, Stream};
use futures::executor::{self, Notify};

// pub(crate)


pub fn timeout<F>(fut: F, timeout: Option<Duration>) -> Result<F::Item, Waited<F::Error>>
where F: Future {
    if let Some(dur) = timeout {
        let start = Instant::now();
        let deadline = start + dur;
        let mut task = executor::spawn(fut);
        let notify = Arc::new(ThreadNotify {
            thread: thread::current(),
        });

        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(Waited::TimedOut);
            }
            match task.poll_future_notify(&notify, 0)? {
                Async::Ready(val) => return Ok(val),
                Async::NotReady => {
                    thread::park_timeout(deadline - now);
                }
            }
        }
    } else {
        fut.wait().map_err(From::from)
    }
}

pub fn stream<S>(stream: S, timeout: Option<Duration>) -> WaitStream<S>
where S: Stream {
    WaitStream {
        stream: executor::spawn(stream),
        timeout: timeout,
    }
}

#[derive(Debug)]
pub enum Waited<E> {
    TimedOut,
    Err(E),
}

impl<E> From<E> for Waited<E> {
    fn from(err: E) -> Waited<E> {
        Waited::Err(err)
    }
}

pub struct WaitStream<S> {
    stream: executor::Spawn<S>,
    timeout: Option<Duration>,
}

impl<S> Iterator for WaitStream<S>
where S: Stream {
    type Item = Result<S::Item, Waited<S::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(dur) = self.timeout {
            let start = Instant::now();
            let deadline = start + dur;
            let notify = Arc::new(ThreadNotify {
                thread: thread::current(),
            });

            loop {
                let now = Instant::now();
                if now >= deadline {
                    return Some(Err(Waited::TimedOut));
                }
                match self.stream.poll_stream_notify(&notify, 0) {
                    Ok(Async::Ready(Some(val))) => return Some(Ok(val)),
                    Ok(Async::Ready(None)) => return None,
                    Ok(Async::NotReady) => {
                        thread::park_timeout(deadline - now);
                    },
                    Err(e) => return Some(Err(Waited::Err(e))),
                }
            }
        } else {
            let notify = Arc::new(ThreadNotify {
                thread: thread::current(),
            });

            loop {
                match self.stream.poll_stream_notify(&notify, 0) {
                    Ok(Async::Ready(Some(val))) => return Some(Ok(val)),
                    Ok(Async::Ready(None)) => return None,
                    Ok(Async::NotReady) => {
                        thread::park();
                    },
                    Err(e) => return Some(Err(Waited::Err(e))),
                }
            }
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
