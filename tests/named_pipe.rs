#![cfg(target_os = "windows")]
#![cfg(not(feature = "rustls-no-provider"))]

use std::convert::Infallible;
use std::time::Duration;

use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};

struct BusyPipe {
    path: String,
    _server: NamedPipeServer,
    _client: NamedPipeClient,
}

fn busy_pipe() -> BusyPipe {
    use std::hash::BuildHasher;

    // libstd uses system random to create each one
    let rng = std::collections::hash_map::RandomState::new();
    let n = rng.hash_one("reqwest-named-pipe");

    let path = format!(r"\\.\pipe\reqwest-test-named-pipe-{}", n);

    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&path)
        .unwrap();

    let client = ClientOptions::new().open(&path).unwrap();

    BusyPipe {
        path,
        _server: server,
        _client: client,
    }
}

#[tokio::test]
async fn wait_for_free_instance() {
    let pipe = busy_pipe();

    let client = reqwest::Client::builder()
        .windows_named_pipe(&*pipe.path)
        .build()
        .unwrap();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;

        let instance = ServerOptions::new().create(&pipe.path).unwrap();
        instance.connect().await.unwrap();

        hyper::server::conn::http1::Builder::new()
            .serve_connection(
                TokioIo::new(instance),
                service_fn(|_| async { Ok::<_, Infallible>(http::Response::new(String::new())) }),
            )
            .await
            .unwrap();
    });

    let result = client
        .get("http://yolo.local/foo")
        .send()
        .await
        .expect("send request");

    assert_eq!(result.status(), 200);
}

#[tokio::test]
async fn pipe_busy_retry_until_timeout() {
    let pipe = busy_pipe();

    let error = reqwest::Client::builder()
        .windows_named_pipe(&*pipe.path)
        .connect_timeout(Duration::from_millis(100))
        .build()
        .unwrap()
        .get("http://yolo.local/foo")
        .send()
        .await
        .unwrap_err();

    assert!(error.is_connect());
    assert!(error.is_timeout());
}
