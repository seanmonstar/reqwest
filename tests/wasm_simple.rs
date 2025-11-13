#![cfg(target_arch = "wasm32")]
use std::time::Duration;

#[cfg(feature = "stream")]
use futures_util::StreamExt;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[wasm_bindgen_test]
async fn simple_example() {
    let res = reqwest::get("https://hyper.rs")
        .await
        .expect("http get example");
    log(&format!("Status: {}", res.status()));

    let body = res.text().await.expect("response to utf-8 text");
    log(&format!("Body:\n\n{body}"));
}

#[wasm_bindgen_test]
async fn request_with_timeout() {
    let client = reqwest::Client::new();
    let err = client
        .get("https://hyper.rs/not-cached")
        .timeout(Duration::from_millis(1))
        .send()
        .await
        .expect_err("Expected error from aborted request");

    assert!(err.is_request());
    assert!(err.is_timeout());
}

#[wasm_bindgen_test]
async fn convert_response_into_http_response_body_bytes() {
    let res = reqwest::get("https://hyper.rs").await.expect("fetch body");

    let http_res: http::Response<reqwest::Body> = res.into();
    let body = http_res.into_body();
    let bytes = body.bytes().await.expect("read body bytes");

    assert!(!bytes.is_empty());
}

#[cfg(feature = "stream")]
#[wasm_bindgen_test]
async fn convert_response_into_http_response_body_stream() {
    let res = reqwest::get("https://hyper.rs")
        .await
        .expect("fetch streaming bytes");

    let http_res: http::Response<reqwest::Body> = res.into();
    let mut stream = http_res.into_body().bytes_stream();
    let mut total = 0usize;

    while let Some(chunk) = stream.next().await {
        total += chunk.expect("stream chunk").len();
    }

    assert!(total > 0);
}
