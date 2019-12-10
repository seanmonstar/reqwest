use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::time::Instant;
use tokio_executor::{
    enter,
    park::{Park, ParkThread, Unpark, UnparkThread},
};

pub(crate) fn timeout<F, I, E>(fut: F, timeout: Option<Duration>) -> Result<I, Waited<E>>
where
    F: Future<Output = Result<I, E>>,
{
    let _entered =
        enter().map_err(|_| Waited::Executor(crate::error::BlockingClientInAsyncContext))?;
    let deadline = timeout.map(|d| {
        log::trace!("wait at most {:?}", d);
        Instant::now() + d
    });

    let mut park = ParkThread::new();
    // Arc shouldn't be necessary, since UnparkThread is reference counted internally,
    // but let's just stay safe for now.
    let waker = futures_util::task::waker(Arc::new(UnparkWaker(park.unpark())));
    let mut cx = Context::from_waker(&waker);

    futures_util::pin_mut!(fut);

    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(val)) => return Ok(val),
            Poll::Ready(Err(err)) => return Err(Waited::Inner(err)),
            Poll::Pending => (), // fallthrough
        }

        if let Some(deadline) = deadline {
            let now = Instant::now();
            if now >= deadline {
                log::trace!("wait timeout exceeded");
                return Err(Waited::TimedOut(crate::error::TimedOut));
            }

            log::trace!("park timeout {:?}", deadline - now);
            park.park_timeout(deadline - now)
                .expect("ParkThread doesn't error");
        } else {
            park.park().expect("ParkThread doesn't error");
        }
    }
}

#[derive(Debug)]
pub(crate) enum Waited<E> {
    TimedOut(crate::error::TimedOut),
    Executor(crate::error::BlockingClientInAsyncContext),
    Inner(E),
}

struct UnparkWaker(UnparkThread);

impl futures_util::task::ArcWake for UnparkWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.0.unpark();
    }
}
