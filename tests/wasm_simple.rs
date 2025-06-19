#![cfg(target_arch = "wasm32")]
use std::time::Duration;

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

#[cfg(all(feature = "stream"))]
#[wasm_bindgen_test]
async fn send_stream() {
    let chunks: Vec<Result<_, ::std::io::Error>> = vec![Ok("hello"), Ok(" "), Ok("world")];
    let stream = futures_util::stream::iter(chunks);
    let client = reqwest::Client::new();
    let resp = client
        .post("https://httpbin.org/post")
        .header("Content-Type", "text/plain")
        .body(reqwest::Body::wrap_stream(stream))
        .duplex("half")
        .send()
        .await
        .unwrap();
    let body = resp.text().await.expect("response to utf-8 text");
    log(&format!("Body:\n\n{body}"));
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
        .get("https://hyper.rs")
        .timeout(Duration::from_millis(1))
        .send()
        .await
        .expect_err("Expected error from aborted request");

    assert!(err.is_request());
    assert!(err.is_timeout());
}
