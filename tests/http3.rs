#![cfg(feature = "http3-no-provider")]
#![cfg(not(target_arch = "wasm32"))]

mod support;

use http::header::CONTENT_LENGTH;
use std::error::Error;
use support::server;

fn assert_send_sync<T: Send + Sync>(_: &T) {}

#[tokio::test]
async fn http3_request_full() {
    use http_body_util::BodyExt;

    let server = server::Http3::new().build(move |req| async move {
        assert_eq!(req.headers()[CONTENT_LENGTH], "5");
        let reqb = req.collect().await.unwrap().to_bytes();
        assert_eq!(reqb, "hello");
        http::Response::default()
    });

    let url = format!("https://{}/content-length", server.addr());
    let res_fut = reqwest::Client::builder()
        .http3_prior_knowledge()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client builder")
        .post(url)
        .version(http::Version::HTTP_3)
        .body("hello")
        .send();

    assert_send_sync(&res_fut);
    let res = res_fut.await.expect("request");

    assert_eq!(res.version(), http::Version::HTTP_3);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

async fn find_free_tcp_addr() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
    listener.local_addr().unwrap()
}

#[cfg(feature = "http3-no-provider")]
#[tokio::test]
async fn http3_test_failed_connection() {
    let addr = find_free_tcp_addr().await;
    let port = addr.port();

    let url = format!("https://[::1]:{port}/");
    let client = reqwest::Client::builder()
        .http3_prior_knowledge()
        .danger_accept_invalid_certs(true)
        .http3_max_idle_timeout(std::time::Duration::from_millis(20))
        .build()
        .expect("client builder");

    let err = client
        .get(&url)
        .version(http::Version::HTTP_3)
        .send()
        .await
        .unwrap_err();

    let err = err
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<quinn::ConnectionError>()
        .unwrap();
    assert_eq!(*err, quinn::ConnectionError::TimedOut);

    let err = client
        .get(&url)
        .version(http::Version::HTTP_3)
        .send()
        .await
        .unwrap_err();

    let err = err
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<quinn::ConnectionError>()
        .unwrap();
    assert_eq!(*err, quinn::ConnectionError::TimedOut);

    let server = server::Http3::new()
        .with_addr(addr)
        .build(|_| async { http::Response::default() });

    let res = client
        .post(&url)
        .version(http::Version::HTTP_3)
        .body("hello")
        .send()
        .await
        .expect("request");

    assert_eq!(res.version(), http::Version::HTTP_3);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    drop(server);
}

#[cfg(feature = "http3-no-provider")]
#[tokio::test]
async fn http3_test_concurrent_request() {
    let server = server::Http3::new().build(|req| async move {
        let mut res = http::Response::default();
        *res.body_mut() = reqwest::Body::from(format!("hello {}", req.uri().path()));
        res
    });
    let addr = server.addr();

    let client = reqwest::Client::builder()
        .http3_prior_knowledge()
        .danger_accept_invalid_certs(true)
        .http3_max_idle_timeout(std::time::Duration::from_millis(20))
        .build()
        .expect("client builder");

    let mut tasks = vec![];
    for i in 0..10 {
        let client = client.clone();
        tasks.push(async move {
            let url = format!("https://{}/{}", addr, i);

            client
                .post(&url)
                .version(http::Version::HTTP_3)
                .send()
                .await
                .expect("request")
        });
    }

    let handlers = tasks.into_iter().map(tokio::spawn).collect::<Vec<_>>();

    for (i, handler) in handlers.into_iter().enumerate() {
        let result = handler.await.unwrap();

        assert_eq!(result.version(), http::Version::HTTP_3);
        assert_eq!(result.status(), reqwest::StatusCode::OK);

        let body = result.text().await.unwrap();
        assert_eq!(body, format!("hello /{}", i));
    }

    drop(server);
}

#[cfg(feature = "http3-no-provider")]
#[tokio::test]
async fn http3_test_reconnection() {
    use std::error::Error;

    use h3::error::{ConnectionError, StreamError};

    let server = server::Http3::new().build(|_| async { http::Response::default() });
    let addr = server.addr();

    let url = format!("https://{}/", addr);
    let client = reqwest::Client::builder()
        .http3_prior_knowledge()
        .danger_accept_invalid_certs(true)
        .http3_max_idle_timeout(std::time::Duration::from_millis(20))
        .build()
        .expect("client builder");

    let res = client
        .post(&url)
        .version(http::Version::HTTP_3)
        .send()
        .await
        .expect("request");

    assert_eq!(res.version(), http::Version::HTTP_3);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    drop(server);

    let err = client
        .get(&url)
        .version(http::Version::HTTP_3)
        .send()
        .await
        .unwrap_err();

    let err = err
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<StreamError>()
        .unwrap();

    assert!(matches!(
        err,
        StreamError::ConnectionError {
            0: ConnectionError::Timeout { .. },
            ..
        }
    ));

    let server = server::Http3::new()
        .with_addr(addr)
        .build(|_| async { http::Response::default() });

    let res = client
        .post(&url)
        .version(http::Version::HTTP_3)
        .body("hello")
        .send()
        .await
        .expect("request");

    assert_eq!(res.version(), http::Version::HTTP_3);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    drop(server);
}

#[cfg(all(feature = "http3-no-provider", feature = "stream"))]
#[tokio::test]
async fn http3_request_stream() {
    use http_body_util::BodyExt;

    let server = server::Http3::new().build(move |req| async move {
        let reqb = req.collect().await.unwrap().to_bytes();
        assert_eq!(reqb, "hello world");
        http::Response::default()
    });

    let url = format!("https://{}", server.addr());
    let body = reqwest::Body::wrap_stream(futures_util::stream::iter(vec![
        Ok::<_, std::convert::Infallible>("hello"),
        Ok::<_, std::convert::Infallible>(" "),
        Ok::<_, std::convert::Infallible>("world"),
    ]));

    let res = reqwest::Client::builder()
        .http3_prior_knowledge()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client builder")
        .post(url)
        .version(http::Version::HTTP_3)
        .body(body)
        .send()
        .await
        .expect("request");

    assert_eq!(res.version(), http::Version::HTTP_3);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

#[cfg(all(feature = "http3-no-provider", feature = "stream"))]
#[tokio::test]
async fn http3_request_stream_error() {
    use http_body_util::BodyExt;

    let server = server::Http3::new().build(move |req| async move {
        // HTTP/3 response can start and finish before the entire request body has been received.
        // To avoid prematurely terminating the session, collect full request body before responding.
        let _ = req.collect().await;

        http::Response::default()
    });

    let url = format!("https://{}", server.addr());
    let body = reqwest::Body::wrap_stream(futures_util::stream::iter(vec![
        Ok::<_, std::io::Error>("first chunk"),
        Err::<_, std::io::Error>(std::io::Error::other("oh no!")),
    ]));

    let res = reqwest::Client::builder()
        .http3_prior_knowledge()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("client builder")
        .post(url)
        .version(http::Version::HTTP_3)
        .body(body)
        .send()
        .await;

    let err = res.unwrap_err();
    assert!(err.is_request());
    let err = err
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<reqwest::Error>()
        .unwrap();
    assert!(err.is_body());
}
