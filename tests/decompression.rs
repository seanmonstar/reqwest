#![cfg(not(target_arch = "wasm32"))]
#![cfg(not(feature = "rustls-no-provider"))]

mod support;

use reqwest::Client;
use support::server;

#[tokio::test]
async fn no_decompression() {
    let client = Client::builder()
        .no_proxy()
        .no_decompression()
        .build()
        .unwrap();

    for encoding in ["gzip", "br", "zstd", "deflate"] {
        for explicit_header in [false, true] {
            let server = server::http(move |req| async move {
                assert_eq!(
                    req.headers()
                        .get("accept-encoding")
                        .map(|v| v.to_str().unwrap()),
                    explicit_header.then_some(encoding),
                );
                // Invalid compressed data detects any attempt to decode the body.
                http::Response::builder()
                    .header("content-encoding", encoding)
                    .header("content-length", "3")
                    .body("raw".into())
                    .unwrap()
            });

            let mut request = client.get(format!("http://{}/", server.addr()));
            if explicit_header {
                request = request.header("accept-encoding", encoding);
            }
            let response = request.send().await.unwrap();
            assert_eq!(response.headers()["content-encoding"], encoding);
            assert_eq!(response.headers()["content-length"], "3");
            assert_eq!(response.bytes().await.unwrap(), "raw");
        }
    }
}
