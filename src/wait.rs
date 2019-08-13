use std::time::Duration;
use futures_timer::TryFutureExt as TryFutureTimerExt;
use futures::future::TryFutureExt;

use futures::{Future, Stream, task};

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
        stream: task::spawn(stream),
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

pub(crate) struct WaitStream<S> {
    stream: task::spawn<S>,
    timeout: Option<Duration>,
}