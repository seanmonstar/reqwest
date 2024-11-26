mod support;
use support::server;

use std::io::Write;
use tokio::io::AsyncWriteExt;
use tokio::time::Duration;

#[tokio::test]
async fn gzip_response() {
    gzip_case(10_000, 4096).await;
}

#[tokio::test]
async fn gzip_single_byte_chunks() {
    gzip_case(10, 1).await;
}

#[tokio::test]
async fn test_gzip_empty_body() {
    let server = server::http(move |req| async move {
        assert_eq!(req.method(), "HEAD");

        http::Response::builder()
            .header("content-encoding", "gzip")
            .body(Default::default())
            .unwrap()
    });

    let client = reqwest::Client::new();
    let res = client
        .head(&format!("http://{}/gzip", server.addr()))
        .send()
        .await
        .unwrap();

    let body = res.text().await.unwrap();

    assert_eq!(body, "");
}

#[tokio::test]
async fn test_accept_header_is_not_changed_if_set() {
    let server = server::http(move |req| async move {
        assert_eq!(req.headers()["accept"], "application/json");
        assert!(req.headers()["accept-encoding"]
            .to_str()
            .unwrap()
            .contains("gzip"));
        http::Response::default()
    });

    let client = reqwest::Client::new();

    let res = client
        .get(&format!("http://{}/accept", server.addr()))
        .header(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn test_accept_encoding_header_is_not_changed_if_set() {
    let server = server::http(move |req| async move {
        assert_eq!(req.headers()["accept"], "*/*");
        assert_eq!(req.headers()["accept-encoding"], "identity");
        http::Response::default()
    });

    let client = reqwest::Client::new();

    let res = client
        .get(&format!("http://{}/accept-encoding", server.addr()))
        .header(
            reqwest::header::ACCEPT_ENCODING,
            reqwest::header::HeaderValue::from_static("identity"),
        )
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

async fn gzip_case(response_size: usize, chunk_size: usize) {
    use futures_util::stream::StreamExt;

    let content: String = (0..response_size)
        .into_iter()
        .map(|i| format!("test {i}"))
        .collect();
    let mut encoder = libflate::gzip::Encoder::new(Vec::new()).unwrap();
    match encoder.write(content.as_bytes()) {
        Ok(n) => assert!(n > 0, "Failed to write to encoder."),
        _ => panic!("Failed to gzip encode string."),
    };

    let gzipped_content = encoder.finish().into_result().unwrap();

    let mut response = format!(
        "\
         HTTP/1.1 200 OK\r\n\
         Server: test-accept\r\n\
         Content-Encoding: gzip\r\n\
         Content-Length: {}\r\n\
         \r\n",
        &gzipped_content.len()
    )
    .into_bytes();
    response.extend(&gzipped_content);

    let server = server::http(move |req| {
        assert!(req.headers()["accept-encoding"]
            .to_str()
            .unwrap()
            .contains("gzip"));

        let gzipped = gzipped_content.clone();
        async move {
            let len = gzipped.len();
            let stream =
                futures_util::stream::unfold((gzipped, 0), move |(gzipped, pos)| async move {
                    let chunk = gzipped.chunks(chunk_size).nth(pos)?.to_vec();

                    Some((chunk, (gzipped, pos + 1)))
                });

            let body = reqwest::Body::wrap_stream(stream.map(Ok::<_, std::convert::Infallible>));

            http::Response::builder()
                .header("content-encoding", "gzip")
                .header("content-length", len)
                .body(body)
                .unwrap()
        }
    });

    let client = reqwest::Client::new();

    let res = client
        .get(&format!("http://{}/gzip", server.addr()))
        .send()
        .await
        .expect("response");

    let body = res.text().await.expect("text");
    assert_eq!(body, content);
}

#[tokio::test]
async fn test_non_chunked_non_fragmented_response() {
    let server = server::low_level_with_response(|_raw_request, client_socket| {
        Box::new(async move {
            let response = b"HTTP/1.1 200 OK\x0d\x0a\
            Content-Type: text/plain\x0d\x0a\
            Connection: keep-alive\x0d\x0a\
            Content-Encoding: gzip\x0d\x0a\
            Content-Length: 85\x0d\x0a\
            \x0d\x0a\
            \x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03\xabV*\xae\xccM\xca\xcfQ\xb2Rr\x0aq\x0e\x0dv\x09Q\xd2Q\xca/H\xcd\xf3\xcc+I-J-.\x01J\x98\x1b\x18\x98\x9a\xe9\x99\x9a\x18\x03\xa5J2sS\x95\xac\x0c\xcd\x8d\x8cM\x8cLML\x0c---j\x01\xd7Gb;D\x00\x00\x00";

            client_socket
                .write_all(response)
                .await
                .expect("response write_all failed");
            client_socket.flush().await.expect("response flush failed");
        })
    });

    let client = reqwest::Client::builder()
        .connection_verbose(true)
        .timeout(Duration::from_secs(15))
        .pool_idle_timeout(Some(std::time::Duration::from_secs(300)))
        .pool_max_idle_per_host(5)
        .build()
        .expect("reqwest client init error");

    let res = client
        .get(&format!("http://{}/", server.addr()))
        .send()
        .await
        .expect("response");

    let body = res.text().await.expect("text");
    assert_eq!(
        body,
        r#"{"symbol":"BTCUSDT","openInterest":"70056.543","time":1723425441998}"#
    );
}

#[tokio::test]
async fn test_chunked_fragmented_response_1() {
    const DELAY_BETWEEN_RESPONSE_PARTS: tokio::time::Duration =
        tokio::time::Duration::from_millis(1000);
    const DELAY_MARGIN: tokio::time::Duration = tokio::time::Duration::from_millis(50);

    let server = server::low_level_with_response(|_raw_request, client_socket| {
        Box::new(async move {
            let response_first_part = b"HTTP/1.1 200 OK\x0d\x0a\
            Content-Type: text/plain\x0d\x0a\
            Transfer-Encoding: chunked\x0d\x0a\
            Connection: keep-alive\x0d\x0a\
            Content-Encoding: gzip\x0d\x0a\
            \x0d\x0a\
            55\x0d\x0a\
            \x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03\xabV*\xae\xccM\xca\xcfQ\xb2Rr\x0aq\x0e\x0dv\x09Q\xd2Q\xca/H\xcd\xf3\xcc+I-J-.\x01J\x98\x1b\x18\x98\x9a\xe9\x99\x9a\x18\x03\xa5J2sS\x95\xac\x0c\xcd\x8d\x8cM\x8cLML\x0c---j\x01\xd7Gb;D\x00\x00\x00";
            let response_second_part = b"\x0d\x0a0\x0d\x0a\x0d\x0a";

            client_socket
                .write_all(response_first_part)
                .await
                .expect("response_first_part write_all failed");
            client_socket
                .flush()
                .await
                .expect("response_first_part flush failed");

            tokio::time::sleep(DELAY_BETWEEN_RESPONSE_PARTS).await;

            client_socket
                .write_all(response_second_part)
                .await
                .expect("response_second_part write_all failed");
            client_socket
                .flush()
                .await
                .expect("response_second_part flush failed");
        })
    });

    let start = tokio::time::Instant::now();

    let client = reqwest::Client::builder()
        .connection_verbose(true)
        .timeout(Duration::from_secs(15))
        .pool_idle_timeout(Some(std::time::Duration::from_secs(300)))
        .pool_max_idle_per_host(5)
        .build()
        .expect("reqwest client init error");

    let res = client
        .get(&format!("http://{}/", server.addr()))
        .send()
        .await
        .expect("response");

    let body = res.text().await.expect("text");
    assert_eq!(
        body,
        r#"{"symbol":"BTCUSDT","openInterest":"70056.543","time":1723425441998}"#
    );
    assert!(start.elapsed() >= DELAY_BETWEEN_RESPONSE_PARTS - DELAY_MARGIN);
}

#[tokio::test]
async fn test_chunked_fragmented_response_2() {
    const DELAY_BETWEEN_RESPONSE_PARTS: tokio::time::Duration =
        tokio::time::Duration::from_millis(1000);
    const DELAY_MARGIN: tokio::time::Duration = tokio::time::Duration::from_millis(50);

    let server = server::low_level_with_response(|_raw_request, client_socket| {
        Box::new(async move {
            let response_first_part = b"HTTP/1.1 200 OK\x0d\x0a\
            Content-Type: text/plain\x0d\x0a\
            Transfer-Encoding: chunked\x0d\x0a\
            Connection: keep-alive\x0d\x0a\
            Content-Encoding: gzip\x0d\x0a\
            \x0d\x0a\
            55\x0d\x0a\
            \x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03\xabV*\xae\xccM\xca\xcfQ\xb2Rr\x0aq\x0e\x0dv\x09Q\xd2Q\xca/H\xcd\xf3\xcc+I-J-.\x01J\x98\x1b\x18\x98\x9a\xe9\x99\x9a\x18\x03\xa5J2sS\x95\xac\x0c\xcd\x8d\x8cM\x8cLML\x0c---j\x01\xd7Gb;D\x00\x00\x00\x0d\x0a";
            let response_second_part = b"0\x0d\x0a\x0d\x0a";

            client_socket
                .write_all(response_first_part)
                .await
                .expect("response_first_part write_all failed");
            client_socket
                .flush()
                .await
                .expect("response_first_part flush failed");

            tokio::time::sleep(DELAY_BETWEEN_RESPONSE_PARTS).await;

            client_socket
                .write_all(response_second_part)
                .await
                .expect("response_second_part write_all failed");
            client_socket
                .flush()
                .await
                .expect("response_second_part flush failed");
        })
    });

    let start = tokio::time::Instant::now();

    let client = reqwest::Client::builder()
        .connection_verbose(true)
        .timeout(Duration::from_secs(15))
        .pool_idle_timeout(Some(std::time::Duration::from_secs(300)))
        .pool_max_idle_per_host(5)
        .build()
        .expect("reqwest client init error");

    let res = client
        .get(&format!("http://{}/", server.addr()))
        .send()
        .await
        .expect("response");

    let body = res.text().await.expect("text");
    assert_eq!(
        body,
        r#"{"symbol":"BTCUSDT","openInterest":"70056.543","time":1723425441998}"#
    );
    assert!(start.elapsed() >= DELAY_BETWEEN_RESPONSE_PARTS - DELAY_MARGIN);
}
