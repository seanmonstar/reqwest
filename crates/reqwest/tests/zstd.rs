mod support;
use support::server;

#[tokio::test]
async fn zstd_response() {
    zstd_case(10_000, 4096).await;
}

#[tokio::test]
async fn zstd_single_byte_chunks() {
    zstd_case(10, 1).await;
}

#[tokio::test]
async fn test_zstd_empty_body() {
    let server = server::http(move |req| async move {
        assert_eq!(req.method(), "HEAD");

        http::Response::builder()
            .header("content-encoding", "zstd")
            .body(Default::default())
            .unwrap()
    });

    let client = reqwest::Client::new();
    let res = client
        .head(&format!("http://{}/zstd", server.addr()))
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
            .contains("zstd"));
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

async fn zstd_case(response_size: usize, chunk_size: usize) {
    use futures_util::stream::StreamExt;

    let content: String = (0..response_size)
        .into_iter()
        .map(|i| format!("test {i}"))
        .collect();

    let zstded_content = zstd_crate::encode_all(content.as_bytes(), 3).unwrap();

    let mut response = format!(
        "\
         HTTP/1.1 200 OK\r\n\
         Server: test-accept\r\n\
         Content-Encoding: zstd\r\n\
         Content-Length: {}\r\n\
         \r\n",
        &zstded_content.len()
    )
    .into_bytes();
    response.extend(&zstded_content);

    let server = server::http(move |req| {
        assert!(req.headers()["accept-encoding"]
            .to_str()
            .unwrap()
            .contains("zstd"));

        let zstded = zstded_content.clone();
        async move {
            let len = zstded.len();
            let stream =
                futures_util::stream::unfold((zstded, 0), move |(zstded, pos)| async move {
                    let chunk = zstded.chunks(chunk_size).nth(pos)?.to_vec();

                    Some((chunk, (zstded, pos + 1)))
                });

            let body = reqwest::Body::wrap_stream(stream.map(Ok::<_, std::convert::Infallible>));

            http::Response::builder()
                .header("content-encoding", "zstd")
                .header("content-length", len)
                .body(body)
                .unwrap()
        }
    });

    let client = reqwest::Client::new();

    let res = client
        .get(&format!("http://{}/zstd", server.addr()))
        .send()
        .await
        .expect("response");

    let body = res.text().await.expect("text");
    assert_eq!(body, content);
}
