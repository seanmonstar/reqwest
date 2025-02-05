#![cfg(not(target_arch = "wasm32"))]

mod support;

use http::header::CONTENT_LENGTH;
use support::server;

#[cfg(feature = "http3")]
#[tokio::test]
async fn http3_request_full() {
    use http_body_util::BodyExt;

    let server = server::http3(move |req| async move {
        assert_eq!(req.headers()[CONTENT_LENGTH], "5");
        let reqb = req.collect().await.unwrap().to_bytes();
        assert_eq!(reqb, "hello");
        http::Response::default()
    });

    let url = format!("https://{}/content-length", server.addr());
    let res = reqwest::Client::builder()
        .http3_prior_knowledge()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client builder")
        .post(url)
        .version(http::Version::HTTP_3)
        .body("hello")
        .send()
        .await
        .expect("request");

    assert_eq!(res.version(), http::Version::HTTP_3);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}
