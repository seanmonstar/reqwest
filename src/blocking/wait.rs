use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::clock;
use tokio_executor::{
    enter,
    park::{Park, ParkThread, Unpark, UnparkThread},
    EnterError,
};

pub(crate) fn timeout<F, I, E>(fut: F, timeout: Option<Duration>) -> Result<I, Waited<E>>
where
    F: Future<Output = Result<I, E>>,
{
    let _entered = enter().map_err(Waited::Executor)?;
    let deadline = timeout.map(|d| {
        log::trace!("wait at most {:?}", d);
        clock::now() + d
    });

    let mut park = ParkThread::new();
    // Arc shouldn't be necessary, since UnparkThread is reference counted internally,
    // but let's just stay safe for now.
    let waker = futures::task::waker(Arc::new(UnparkWaker(park.unpark())));
    let mut cx = Context::from_waker(&waker);

    futures::pin_mut!(fut);

    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(val)) => return Ok(val),
            Poll::Ready(Err(err)) => return Err(Waited::Inner(err)),
            Poll::Pending => (), // fallthrough
        }

        if let Some(deadline) = deadline {
            let now = clock::now();
            if now >= deadline {
                log::trace!("wait timeout exceeded");
                return Err(Waited::TimedOut);
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
    TimedOut,
    Executor(EnterError),
    Inner(E),
}

struct UnparkWaker(UnparkThread);

impl futures::task::ArcWake for UnparkWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.0.unpark();
    }
}
