use std::{
    pin::Pin,
    task::{ready, Context, Poll},
};

use futures_core::Future;

use crate::{
    wasm::component::bindings::wasi::{
        self,
        http::{
            outgoing_handler::{FutureIncomingResponse, OutgoingRequest},
            types::{OutgoingBody, OutputStream},
        },
    },
    Body, Request, Response,
};

#[derive(Debug)]
pub struct ResponseFuture {
    request: Request,
    state: RequestState,
}

impl ResponseFuture {
    pub fn new(mut request: Request, outgoing_request: OutgoingRequest) -> crate::Result<Self> {
        let state = match request.body_mut().take() {
            Some(body) => {
                let Ok(outgoing_body) = outgoing_request.body() else {
                    return Err(crate::error::request("outgoing body error"));
                };

                let Ok(stream) = outgoing_body.write() else {
                    return Err(crate::error::request("outgoing body write error"));
                };

                match wasi::http::outgoing_handler::handle(outgoing_request, None) {
                    Ok(future) => RequestState::Write(RequestWriteState {
                        response_future: Some(future),
                        outgoing_body: Some(outgoing_body),
                        stream: Some(stream),
                        body,
                        bytes_written: 0,
                    }),
                    Err(e) => return Err(crate::error::request("request error")),
                }
            }
            None => match wasi::http::outgoing_handler::handle(outgoing_request, None) {
                Ok(future) => RequestState::Response(future),
                Err(e) => return Err(crate::error::request("request error")),
            },
        };

        Ok(Self { request, state })
    }
}

impl Future for ResponseFuture {
    type Output = crate::Result<Response>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        match &mut this.state {
            RequestState::Write(write_state) => match ready!(Pin::new(write_state).poll(cx)) {
                Ok(future) => {
                    this.state = RequestState::Response(future);
                    Pin::new(this).poll(cx)
                }
                Err(e) => return Poll::Ready(Err(e)),
            },
            RequestState::Response(future) => {
                if !future.subscribe().ready() {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }

                let result = match future.get() {
                    None => Err(crate::error::request("http request response missing")),
                    // Shouldn't occur
                    Some(Err(_)) => Err(crate::error::request(
                        "http request response requested more than once",
                    )),
                    Some(Ok(response)) => response.map_err(crate::error::request),
                };

                match result {
                    Ok(response) => Poll::Ready(Ok(Response::new(
                        http::Response::new(response),
                        this.request.url().clone(),
                    ))),
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
        }
    }
}

#[derive(Debug)]
enum RequestState {
    Write(RequestWriteState),
    Response(FutureIncomingResponse),
}

#[derive(Debug)]
struct RequestWriteState {
    response_future: Option<FutureIncomingResponse>,
    outgoing_body: Option<OutgoingBody>,
    stream: Option<OutputStream>,
    body: Body,
    bytes_written: u64,
}

impl Future for RequestWriteState {
    type Output = crate::Result<FutureIncomingResponse>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // we need this by-value, so we must take care this
        // is always some.
        let stream = this.stream.take().expect("state error");

        // will be none if the body is a stream, but we are
        // sending a request which means we already stored a set
        // of bytes here
        let bytes = this.body.as_bytes().expect("never none during a request");

        // stream is ready when all data is flushed, and if we wrote all the bytes we
        // are ready to continue.
        if this.bytes_written == bytes.len() as u64 {
            if stream.flush().is_err() {
                return Poll::Ready(Err(crate::error::request(
                    "outgoing body write flush error",
                )));
            }

            if stream.subscribe().ready() {
                // will trap if not dropped before body
                drop(stream);

                let future = this.response_future.take().expect("state error");
                let outgoing_body = this.outgoing_body.take().expect("state error");

                if OutgoingBody::finish(outgoing_body, None).is_err() {
                    return Poll::Ready(Err(crate::error::request("request error")));
                }

                return Poll::Ready(Ok(future));
            } else {
                this.stream.insert(stream);
                cx.waker().wake_by_ref();

                return Poll::Pending;
            }
        } else if !stream.subscribe().ready() {
            this.stream.insert(stream);
            cx.waker().wake_by_ref();

            return Poll::Pending;
        }

        let Ok(bytes_to_write) = stream
            .check_write()
            .map(|len| len.min(bytes.len() as u64 - this.bytes_written))
        else {
            return Poll::Ready(Err(crate::error::request(
                "outgoing body write check write error",
            )));
        };

        let next_write_block =
            (this.bytes_written as usize)..(this.bytes_written as usize + bytes_to_write as usize);

        if let Err(_) = stream.write(&bytes[next_write_block]) {
            return Poll::Ready(Err(crate::error::request(
                "outgoing body write bytes error",
            )));
        };

        this.bytes_written += bytes_to_write;
        this.stream.insert(stream);

        if this.bytes_written != bytes.len() as u64 {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        } else {
            Pin::new(this).poll(cx)
        }
    }
}
