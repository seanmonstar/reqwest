#![cfg(not(target_arch = "wasm32"))]
#![cfg(not(feature = "rustls-no-provider"))]

use std::{io::ErrorKind, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

#[tokio::test]
async fn zero_linger_preserves_complete_transfer_and_resets_on_close() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut received = Vec::new();
        let mut buffer = [0u8; 32768];
        let header_len = loop {
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            received.extend_from_slice(&buffer[..n]);
            if let Some(end) = received.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = String::from_utf8_lossy(&received[..header_len]).to_ascii_lowercase();
        assert!(headers.contains("content-length: 2097152"));
        while received.len() - header_len < 2 * 1024 * 1024 {
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            received.extend_from_slice(&buffer[..n]);
        }
        assert!(received[header_len..].iter().all(|byte| *byte == b'U'));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3145728\r\n\r\n")
            .await
            .unwrap();
        stream
            .write_all(&vec![b'R'; 3 * 1024 * 1024])
            .await
            .unwrap();
        let result = timeout(Duration::from_secs(5), stream.read(&mut buffer))
            .await
            .unwrap();
        assert!(result.is_err_and(|error| error.kind() == ErrorKind::ConnectionReset));
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .tcp_linger(Duration::ZERO)
        .build()
        .unwrap();
    let response = client
        .post(format!("http://{address}/upload"))
        .body(vec![b'U'; 2 * 1024 * 1024])
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(response.len(), 3 * 1024 * 1024);
    assert!(response.iter().all(|byte| *byte == b'R'));
    drop(client);
    server.await.unwrap();
}

#[cfg(feature = "blocking")]
#[test]
fn blocking_builder_accepts_tcp_linger() {
    reqwest::blocking::Client::builder()
        .tcp_linger(Duration::ZERO)
        .tcp_linger(None)
        .build()
        .unwrap();
}
