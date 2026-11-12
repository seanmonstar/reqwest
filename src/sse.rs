use bytes::{Bytes, BytesMut};
use futures_core::Stream;
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A Server-Sent Events message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The event data payload.
    pub data: String,
    /// The optional event name.
    pub event: Option<String>,
    /// The optional event id.
    pub id: Option<String>,
    /// The optional retry value, in milliseconds.
    pub retry: Option<u64>,
}

#[derive(Debug)]
struct EventState {
    data: String,
    event: Option<String>,
    id: Option<String>,
    retry: Option<u64>,
}

impl EventState {
    fn new() -> Self {
        Self {
            data: String::new(),
            event: None,
            id: None,
            retry: None,
        }
    }

    fn reset(&mut self) {
        self.data.clear();
        self.event = None;
        self.id = None;
        self.retry = None;
    }

    fn build_event(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() {
            self.reset();
            return None;
        }

        Some(SseEvent {
            data: std::mem::take(&mut self.data),
            event: self.event.take(),
            id: self.id.take(),
            retry: self.retry.take(),
        })
    }

    fn push_data(&mut self, value: &str) {
        if !self.data.is_empty() {
            self.data.push('\n');
        }
        self.data.push_str(value);
    }
}

pin_project! {
    /// A stream adapter that parses Server-Sent Events from a byte stream.
    #[derive(Debug)]
    pub struct SseStream<S> {
        #[pin]
        inner: S,
        buffer: BytesMut,
        state: EventState,
        done: bool,
    }
}

impl<S> SseStream<S>
where
    S: Stream<Item = crate::Result<Bytes>>,
{
    /// Create a new SSE stream from a bytes stream.
    pub fn new(stream: S) -> Self {
        Self {
            inner: stream,
            buffer: BytesMut::new(),
            state: EventState::new(),
            done: false,
        }
    }

    fn process_line(state: &mut EventState, line: &[u8]) -> crate::Result<Option<SseEvent>> {
        if line.is_empty() {
            return Ok(state.build_event());
        }

        if line[0] == b':' {
            return Ok(None);
        }

        let (field_bytes, value_bytes) = match line.iter().position(|&b| b == b':') {
            Some(idx) => {
                let mut value = &line[idx + 1..];
                if value.first() == Some(&b' ') {
                    value = &value[1..];
                }
                (&line[..idx], value)
            }
            None => (line, &b""[..]),
        };

        let field = std::str::from_utf8(field_bytes).map_err(crate::error::decode)?;
        let value = std::str::from_utf8(value_bytes).map_err(crate::error::decode)?;

        match field {
            "data" => state.push_data(value),
            "event" => state.event = Some(value.to_string()),
            "id" => {
                if !value.contains('\0') {
                    state.id = Some(value.to_string());
                }
            }
            "retry" => {
                if let Ok(retry) = value.parse::<u64>() {
                    state.retry = Some(retry);
                }
            }
            _ => {}
        }

        Ok(None)
    }

    fn poll_event_from_buffer(
        state: &mut EventState,
        buffer: &mut BytesMut,
    ) -> crate::Result<Option<SseEvent>> {
        loop {
            let newline = buffer.iter().position(|&b| b == b'\n');
            let Some(pos) = newline else {
                return Ok(None);
            };

            let line_bytes = buffer.split_to(pos + 1);
            let mut line = &line_bytes[..pos];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }

            if let Some(event) = Self::process_line(state, line)? {
                return Ok(Some(event));
            }
        }
    }
}

impl<S> Stream for SseStream<S>
where
    S: Stream<Item = crate::Result<Bytes>>,
{
    type Item = crate::Result<SseEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if *this.done {
            return Poll::Ready(None);
        }

        loop {
            match Self::poll_event_from_buffer(this.state, this.buffer) {
                Ok(Some(event)) => return Poll::Ready(Some(Ok(event))),
                Ok(None) => {}
                Err(err) => {
                    *this.done = true;
                    return Poll::Ready(Some(Err(err)));
                }
            }

            match this.inner.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => {
                    this.buffer.extend_from_slice(&chunk);
                }
                Poll::Ready(Some(Err(err))) => {
                    *this.done = true;
                    return Poll::Ready(Some(Err(err)));
                }
                Poll::Ready(None) => {
                    *this.done = true;
                    if let Some(event) = this.state.build_event() {
                        return Poll::Ready(Some(Ok(event)));
                    }
                    return Poll::Ready(None);
                }
            }
        }
    }
}
