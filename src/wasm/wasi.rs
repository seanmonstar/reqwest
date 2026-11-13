//! The transport on WASI: requests go out through `wasi:http/client` 0.3, in
//! place of the browser's `fetch`. The host makes the connection, and TLS.

use std::fmt;
use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures_util::future::{select, Either};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use http::HeaderValue;
use http_body::Body as _;
use wasip3::clocks::monotonic_clock;
use wasip3::http_compat::{http_from_wasi_response, http_into_wasi_request, IncomingResponseBody};

use super::{Request, Response};

pub(super) async fn fetch(req: Request) -> crate::Result<Response> {
    let deadline = req.timeout().map(Deadline::after);
    let mut url = req.url().clone();
    let (body, content_type) = match req.body() {
        Some(body) if !body.is_empty() => body.to_bytes(),
        _ => (None, None),
    };
    // A fragment is not part of what is sent.
    url.set_fragment(None);
    let mut request = http::Request::builder()
        .method(req.method().clone())
        .uri(url.as_str())
        .body(http_body_util::Full::new(body.clone().unwrap_or_default()))
        .map_err(crate::error::builder)?;
    *request.headers_mut() = req.headers().clone();
    if let Some(content_type) = content_type {
        request
            .headers_mut()
            .entry(CONTENT_TYPE)
            .or_insert(content_type);
    }
    // Without a length, the body would be sent chunked.
    if let Some(body) = &body {
        request
            .headers_mut()
            .entry(CONTENT_LENGTH)
            .or_insert_with(|| HeaderValue::from(body.len()));
    }

    let request = http_into_wasi_request(request)
        .map_err(error_code)
        .map_err(crate::error::builder)?;
    let sent = wasip3::http::client::send(request);
    let response = match deadline {
        Some(deadline) => deadline.within(sent).await?,
        None => sent.await,
    }
    .map_err(error_code)
    .map_err(|e| crate::error::request(e).with_url(req.url().clone()))?;
    let response = http_from_wasi_response(response)
        .map_err(error_code)
        .map_err(crate::error::request)?;
    let response = response.map(|body| Incoming {
        body,
        deadline,
        timer: None,
    });
    Ok(Response::new(response, req.url().clone()))
}

/// A response body as it arrives, within the request's timeout.
pub(super) struct Incoming {
    body: IncomingResponseBody,
    deadline: Option<Deadline>,
    timer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl Incoming {
    /// The next chunk of data; trailers are skipped.
    pub(super) fn poll_data(&mut self, cx: &mut Context<'_>) -> Poll<Option<crate::Result<Bytes>>> {
        loop {
            match Pin::new(&mut self.body).poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        return Poll::Ready(Some(Ok(data)));
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(crate::error::decode(error_code(e)))))
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => break,
            }
        }
        if let Some(Deadline(when)) = self.deadline {
            let timer = self
                .timer
                .get_or_insert_with(|| Box::pin(monotonic_clock::wait_until(when)));
            if timer.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Some(Err(crate::error::body(crate::error::TimedOut))));
            }
        }
        Poll::Pending
    }

    /// The next chunk of data.
    pub(super) async fn data(&mut self) -> Option<crate::Result<Bytes>> {
        std::future::poll_fn(|cx| self.poll_data(cx)).await
    }

    /// All of the remaining data.
    pub(super) async fn collect(&mut self) -> crate::Result<Bytes> {
        let mut all = Vec::new();
        while let Some(chunk) = self.data().await {
            all.extend_from_slice(&chunk?);
        }
        Ok(all.into())
    }
}

impl futures_core::Stream for Incoming {
    type Item = crate::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_data(cx)
    }
}

impl fmt::Debug for Incoming {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Incoming").finish()
    }
}

/// When a request's timeout runs out, on the monotonic clock.
#[derive(Clone, Copy)]
struct Deadline(monotonic_clock::Mark);

impl Deadline {
    fn after(timeout: &Duration) -> Self {
        let nanos = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);
        Deadline(monotonic_clock::now().saturating_add(nanos))
    }

    async fn within<F: Future>(self, future: F) -> crate::Result<F::Output> {
        match select(pin!(future), pin!(monotonic_clock::wait_until(self.0))).await {
            Either::Left((output, _)) => Ok(output),
            Either::Right(_) => Err(crate::error::request(crate::error::TimedOut)),
        }
    }
}

fn error_code(e: wasip3::http::types::ErrorCode) -> crate::error::BoxError {
    format!("{e:?}").into()
}

