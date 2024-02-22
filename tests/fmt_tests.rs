#![cfg(not(target_arch = "wasm32"))]

// cover [fmt] BRANCH 2
#[tokio::test]
async fn test_fmt_request_error_msg() {
    let url = "https://hyper.rs/prox";
    let proxy = String::from("https://localhost");

    let res = reqwest::Client::builder()
        .proxy(reqwest::Proxy::https(&proxy).unwrap())
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
        .unwrap_err();

    let assert = res.to_string().contains("error sending request");
    assert!(assert);
}