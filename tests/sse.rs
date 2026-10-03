#![cfg(not(target_arch = "wasm32"))]
mod support;

use bytes::Bytes;
use futures_util::stream;
use futures_util::StreamExt;
use http::header::CONTENT_TYPE;
use support::server;

#[tokio::test]
async fn sse_parses_events() {
    let server = server::http(|_req| async move {
        let chunks = vec![
            Ok::<_, std::io::Error>(Bytes::from_static(b"event: greeting\n")),
            Ok(Bytes::from_static(b"data: hello\n")),
            Ok(Bytes::from_static(b"data: world\n")),
            Ok(Bytes::from_static(b"id: 123\nretry: 5000\n\n")),
            Ok(Bytes::from_static(b":comment\n\n")),
            Ok(Bytes::from_static(b"data: last\n\n")),
        ];
        let body = reqwest::Body::wrap_stream(stream::iter(chunks));
        http::Response::builder()
            .header(CONTENT_TYPE, "text/event-stream")
            .body(body)
            .unwrap()
    });

    let url = format!("http://{}/sse", server.addr());
    let mut events = reqwest::get(url).await.unwrap().sse();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "hello\nworld");
    assert_eq!(first.event.as_deref(), Some("greeting"));
    assert_eq!(first.id.as_deref(), Some("123"));
    assert_eq!(first.retry, Some(5000));

    let second = events.next().await.unwrap().unwrap();
    assert_eq!(second.data, "last");
    assert_eq!(second.event, None);
    assert_eq!(second.id, None);
    assert_eq!(second.retry, None);

    assert!(events.next().await.is_none());
}
