use std::future::{poll_fn, Future};
use std::pin::{pin, Pin};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderValue, Method};
use http_body::Frame;
use http_body_util::Full;
use percent_encoding::percent_decode_str;
use url::Url;
use wasip3::clocks::monotonic_clock;
use wasip3::http::types::{Request as WasiRequest, Response as WasiResponse};
use wasip3::http_compat::{http_from_wasi_response, http_into_wasi_request, IncomingResponseBody};

use super::{Request, Response};

pub(super) async fn fetch(req: Request) -> crate::Result<Response> {
    let deadline = req.timeout().map(|timeout| Deadline::after(*timeout));
    let mut url = req.url().clone();
    let credentials = take_credentials(&mut url);
    let request = to_wasi_request(&req, &url, credentials)?;
    let response = send(request, deadline)
        .await
        .map_err(|e| e.with_url(url.clone()))?;
    let response = http_from_wasi_response(response)
        .map_err(|e| crate::error::request(e).with_url(url.clone()))?;
    let response = response.map(|body| Incoming {
        body,
        deadline,
        timer: None,
        ended: false,
    });
    Ok(Response::new(response, url))
}

fn take_credentials(url: &mut Url) -> Option<HeaderValue> {
    let username = percent_decode_str(url.username()).decode_utf8().ok()?;
    let password = url
        .password()
        .and_then(|password| percent_decode_str(password).decode_utf8().ok());
    if username.is_empty() && password.is_none() {
        return None;
    }
    let authorization = crate::util::basic_auth(username, password);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Some(authorization)
}

async fn send(request: WasiRequest, deadline: Option<Deadline>) -> crate::Result<WasiResponse> {
    let sent = wasip3::http::client::send(request);
    let result = match deadline {
        Some(deadline) => deadline
            .within(sent)
            .await
            .ok_or_else(|| crate::error::request(crate::error::TimedOut))?,
        None => sent.await,
    };
    result.map_err(crate::error::request)
}

fn to_wasi_request(
    req: &Request,
    url: &Url,
    credentials: Option<HeaderValue>,
) -> crate::Result<WasiRequest> {
    let mut target = url.clone();
    target.set_fragment(None);
    let uri = http::Uri::try_from(target.as_str())
        .map_err(|_| crate::error::url_invalid_uri(url.clone()))?;

    let (body, content_type) = match req.body() {
        Some(body) if !body.is_empty() => {
            let (body, content_type) = body.to_bytes();
            (Some(body), content_type)
        }
        _ => (None, None),
    };

    let mut headers = req.headers().clone();
    headers
        .entry(ACCEPT)
        .or_insert(HeaderValue::from_static("*/*"));
    if let Some(content_type) = content_type {
        headers.insert(CONTENT_TYPE, content_type);
    }
    if let Some(credentials) = credentials {
        headers.entry(AUTHORIZATION).or_insert(credentials);
    }
    match &body {
        Some(body) => {
            headers
                .entry(CONTENT_LENGTH)
                .or_insert_with(|| HeaderValue::from(body.len()));
        }
        None if matches!(*req.method(), Method::POST | Method::PUT | Method::PATCH) => {
            headers
                .entry(CONTENT_LENGTH)
                .or_insert(HeaderValue::from_static("0"));
        }
        None => {}
    }

    let mut request = http::Request::builder()
        .method(req.method().clone())
        .uri(uri)
        .body(Full::new(body.unwrap_or_default()))
        .map_err(|e| crate::error::builder(e).with_url(url.clone()))?;
    *request.headers_mut() = headers;
    http_into_wasi_request(request).map_err(|e| crate::error::builder(e).with_url(url.clone()))
}

#[derive(Clone, Copy)]
struct Deadline(monotonic_clock::Mark);

impl Deadline {
    fn after(timeout: Duration) -> Self {
        let nanos = u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX);
        Deadline(monotonic_clock::now().saturating_add(nanos))
    }

    fn is_over(self) -> bool {
        monotonic_clock::now() >= self.0
    }

    async fn within<F: Future>(self, future: F) -> Option<F::Output> {
        let mut future = pin!(future);
        let mut timer = pin!(monotonic_clock::wait_until(self.0));
        poll_fn(|cx| {
            if let Poll::Ready(output) = future.as_mut().poll(cx) {
                return Poll::Ready(Some(output));
            }
            if timer.as_mut().poll(cx).is_ready() {
                return Poll::Ready(None);
            }
            Poll::Pending
        })
        .await
    }
}

pub(super) struct Incoming {
    body: IncomingResponseBody,
    deadline: Option<Deadline>,
    timer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    ended: bool,
}

impl http_body::Body for Incoming {
    type Data = Bytes;
    type Error = crate::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, crate::Error>>> {
        let this = &mut *self;
        if this.ended {
            return Poll::Ready(None);
        }
        if let Some(deadline) = this.deadline {
            if deadline.is_over() {
                return this.end(Some(crate::error::body(crate::error::TimedOut)));
            }
        }
        match Pin::new(&mut this.body).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => return Poll::Ready(Some(Ok(frame))),
            Poll::Ready(Some(Err(code))) => return this.end(Some(crate::error::body(code))),
            Poll::Ready(None) => return this.end(None),
            Poll::Pending => {}
        }
        if let Some(deadline) = this.deadline {
            let timer = this
                .timer
                .get_or_insert_with(|| Box::pin(monotonic_clock::wait_until(deadline.0)));
            if timer.as_mut().poll(cx).is_ready() {
                return this.end(Some(crate::error::body(crate::error::TimedOut)));
            }
        }
        Poll::Pending
    }
}

impl Incoming {
    fn end(
        &mut self,
        error: Option<crate::Error>,
    ) -> Poll<Option<Result<Frame<Bytes>, crate::Error>>> {
        self.ended = true;
        self.timer = None;
        Poll::Ready(error.map(Err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_taken_from_the_url() {
        let mut url = Url::parse("http://us%40er:pa:ss@example.com/path?q=1").unwrap();
        let authorization = take_credentials(&mut url).unwrap();
        assert_eq!(authorization, "Basic dXNAZXI6cGE6c3M=");
        assert!(authorization.is_sensitive());
        assert_eq!(url.as_str(), "http://example.com/path?q=1");
    }

    #[test]
    fn a_url_without_credentials_is_unchanged() {
        let mut url = Url::parse("http://example.com/path").unwrap();
        assert!(take_credentials(&mut url).is_none());
        assert_eq!(url.as_str(), "http://example.com/path");
    }
}
