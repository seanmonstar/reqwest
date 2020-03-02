mod support;
use support::*;

#[cfg(feature = "rustls-tls")]
#[tokio::test]
async fn http2_client_server_rustls_tls() {
    extern crate rustls;

    let server = server::http2(move |_req| async { http::Response::new("Hello".into()) });

    let tls = rustls::ClientConfig::new();

    let client = reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .http2_prior_knowledge()
        .build()
        .expect("preconfigured rustls tls");

    let res = client
        .get(&format!("http://{}/text", server.addr()))
        .send()
        .await
        .expect("Failed to get");
    let text = res.text().await.expect("Failed to get text");
    assert_eq!("Hello", text);
}

#[cfg(feature = "native-tls")]
#[tokio::test]
async fn http2_client_server_native_tls() {
    let server = server::http2(move |_req| async { http::Response::new("Hello".into()) });

    let client = reqwest::Client::builder()
        .http2_prior_knowledge()
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("preconfigured rustls tls");

    let res = client
        .get(&format!("http://{}/text", server.addr()))
        .send()
        .await
        .expect("Failed to get");
    let text = res.text().await.expect("Failed to get text");
    assert_eq!("Hello", text);
}
