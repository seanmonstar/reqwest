#![cfg(all(feature = "__rustls", not(target_arch = "wasm32")))]
#![cfg(not(feature = "rustls-no-provider"))]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcgen::CertifiedKey;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

fn certificate(name: &str) -> CertifiedKey {
    rcgen::generate_simple_self_signed(vec![name.into()]).unwrap()
}

fn roots(certs: &[CertificateDer<'static>]) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots.add(cert.clone()).unwrap();
    }
    roots
}

fn server_config(
    identity: &CertifiedKey,
    clients: &[CertificateDer<'static>],
    optional: bool,
) -> Arc<ServerConfig> {
    let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots(clients)));
    let verifier = if optional {
        verifier.allow_unauthenticated()
    } else {
        verifier
    };
    Arc::new(
        ServerConfig::builder()
            .with_client_cert_verifier(verifier.build().unwrap())
            .with_single_cert(
                vec![identity.cert.der().clone()],
                PrivatePkcs8KeyDer::from(identity.key_pair.serialize_der()).into(),
            )
            .unwrap(),
    )
}

fn client_config(proxy: &CertifiedKey, client: &CertifiedKey) -> Arc<ClientConfig> {
    let mut config = ClientConfig::builder()
        .with_root_certificates(roots(&[proxy.cert.der().clone()]))
        .with_client_auth_cert(
            vec![client.cert.der().clone()],
            PrivatePkcs8KeyDer::from(client.key_pair.serialize_der()).into(),
        )
        .unwrap();
    // The proxy connection must remain HTTP/1 even with this configuration.
    config.alpn_protocols = vec![b"h2".to_vec()];
    Arc::new(config)
}

async fn headers(stream: &mut (impl AsyncRead + Unpin)) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await.unwrap());
        assert!(bytes.len() < 16384);
    }
    String::from_utf8(bytes).unwrap()
}

// Terminate the destination TLS inside the tunnel to inspect both handshakes.
async fn proxy(
    config: Arc<ServerConfig>,
    clients: Vec<CertificateDer<'static>>,
    origin: Option<Arc<ServerConfig>>,
) -> (String, JoinHandle<Vec<bool>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(config);
        let mut accepted = Vec::new();
        for client in clients {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(stream).await.unwrap();
            assert_eq!(stream.get_ref().1.peer_certificates().unwrap(), &[client]);
            assert!(stream.get_ref().1.alpn_protocol().is_none());
            let request = headers(&mut stream).await;
            if let Some(origin) = &origin {
                assert!(request.starts_with("CONNECT origin.test:443 HTTP/1.1\r\n"));
                stream
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .unwrap();
                match TlsAcceptor::from(origin.clone()).accept(stream).await {
                    Ok(mut stream) => {
                        // The origin trusts the proxy's client certificates and asks
                        // for one, but it must not receive the proxy identity.
                        assert!(stream.get_ref().1.peer_certificates().is_none());
                        assert!(headers(&mut stream).await.starts_with("GET / HTTP/1.1\r\n"));
                        stream.write_all(RESPONSE).await.unwrap();
                        accepted.push(true);
                    }
                    Err(_) => accepted.push(false),
                }
            } else {
                assert!(request.starts_with("GET http://origin.test/ HTTP/1.1\r\n"));
                stream.write_all(RESPONSE).await.unwrap();
                accepted.push(true);
            }
        }
        accepted
    });
    (url, task)
}

fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .tls_backend_rustls()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(0)
}

async fn finish(task: JoinHandle<Vec<bool>>) -> Vec<bool> {
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn https_proxy_forward_http() {
    let proxy_cert = certificate("127.0.0.1");
    let client_cert = certificate("client.test");
    let clients = vec![client_cert.cert.der().clone()];
    let (url, task) = proxy(server_config(&proxy_cert, &clients, false), clients, None).await;
    let tls = client_config(&proxy_cert, &client_cert);
    let expected = url.parse::<http::Uri>().unwrap();
    let client = builder()
        .proxy(reqwest::Proxy::all(&url).unwrap())
        .https_proxy_tls_config(move |uri| {
            assert_eq!(uri, &expected);
            Ok(tls.clone())
        })
        .build()
        .unwrap();
    let response = client.get("http://origin.test/").send().await.unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
    assert_eq!(finish(task).await, [true]);
}

async fn connect(trust_origin: bool) {
    let proxy_cert = certificate("127.0.0.1");
    let client_cert = certificate("client.test");
    let origin_cert = certificate("origin.test");
    let clients = vec![client_cert.cert.der().clone()];
    let origin = server_config(&origin_cert, &clients, true);
    let (url, task) = proxy(
        server_config(&proxy_cert, &clients, false),
        clients,
        Some(origin),
    )
    .await;
    let tls = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots(&[
                proxy_cert.cert.der().clone(),
                origin_cert.cert.der().clone(),
            ]))
            .with_client_auth_cert(
                vec![client_cert.cert.der().clone()],
                PrivatePkcs8KeyDer::from(client_cert.key_pair.serialize_der()).into(),
            )
            .unwrap(),
    );
    let origin_roots = if trust_origin {
        roots(&[origin_cert.cert.der().clone()])
    } else {
        // Trusting the origin in the proxy config must not make it trusted here.
        RootCertStore::empty()
    };
    let client = builder()
        .proxy(reqwest::Proxy::all(&url).unwrap())
        .tls_backend_preconfigured(
            ClientConfig::builder()
                .with_root_certificates(origin_roots)
                .with_no_client_auth(),
        )
        .https_proxy_tls_config(move |_| Ok(tls.clone()))
        .build()
        .unwrap();
    let response = client.get("https://origin.test/").send().await;
    if trust_origin {
        assert_eq!(response.unwrap().text().await.unwrap(), "ok");
    } else {
        assert!(response.unwrap_err().is_connect());
    }
    assert_eq!(finish(task).await, [trust_origin]);
}

#[tokio::test]
async fn https_proxy_connect_keeps_destination_tls_separate() {
    connect(true).await;
}

#[tokio::test]
async fn https_proxy_does_not_disable_destination_verification() {
    connect(false).await;
}

#[tokio::test]
async fn https_proxy_refreshes_client_certificate() {
    let proxy_cert = certificate("127.0.0.1");
    let first = certificate("first.test");
    let second = certificate("second.test");
    let clients = vec![first.cert.der().clone(), second.cert.der().clone()];
    let (url, task) = proxy(server_config(&proxy_cert, &clients, false), clients, None).await;
    let current = Arc::new(Mutex::new(client_config(&proxy_cert, &first)));
    let factory = current.clone();
    let client = builder()
        .proxy(reqwest::Proxy::all(url).unwrap())
        .https_proxy_tls_config(move |_| Ok(factory.lock().unwrap().clone()))
        .build()
        .unwrap();
    for identity in [&first, &second] {
        *current.lock().unwrap() = client_config(&proxy_cert, identity);
        let response = client.get("http://origin.test/").send().await.unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
    }
    assert_eq!(finish(task).await, [true, true]);
}

#[tokio::test]
async fn https_proxy_factory_error_fails_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = builder()
        .proxy(reqwest::Proxy::all(format!("https://{}", listener.local_addr().unwrap())).unwrap())
        .https_proxy_tls_config(|_| Err("proxy certificate unavailable".into()))
        .build()
        .unwrap();
    for url in ["http://origin.test/", "https://origin.test/"] {
        let error = client.get(url).send().await.unwrap_err();
        assert!(error.is_connect());
        let mut source: &dyn std::error::Error = &error;
        while let Some(next) = source.source() {
            source = next;
        }
        assert_eq!(source.to_string(), "proxy certificate unavailable");
    }
}

#[tokio::test]
async fn https_proxy_verifies_proxy_certificate() {
    for wrong_name in [false, true] {
        let proxy_cert = certificate(if wrong_name {
            "wrong.test"
        } else {
            "127.0.0.1"
        });
        let other_cert = certificate("127.0.0.1");
        let client_cert = certificate("client.test");
        let server_tls = server_config(&proxy_cert, &[client_cert.cert.der().clone()], false);
        let tls = client_config(
            if wrong_name { &proxy_cert } else { &other_cert },
            &client_cert,
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            assert!(TlsAcceptor::from(server_tls).accept(stream).await.is_err());
            vec![false]
        });
        let client = builder()
            .proxy(reqwest::Proxy::all(url).unwrap())
            .https_proxy_tls_config(move |_| Ok(tls.clone()))
            .build()
            .unwrap();
        assert!(client
            .get("http://origin.test/")
            .send()
            .await
            .unwrap_err()
            .is_connect());
        assert_eq!(finish(task).await, [false]);
    }
}

#[tokio::test]
async fn https_proxy_factory_is_not_used_for_http_proxy() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(headers(&mut stream)
            .await
            .starts_with("GET http://origin.test/ HTTP/1.1\r\n"));
        stream.write_all(RESPONSE).await.unwrap();
        vec![true]
    });
    let client = builder()
        .proxy(reqwest::Proxy::all(url).unwrap())
        .https_proxy_tls_config(|_| panic!("not an HTTPS proxy"))
        .build()
        .unwrap();
    assert_eq!(
        client
            .get("http://origin.test/")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    assert_eq!(finish(server).await, [true]);
}

#[cfg(feature = "__native-tls")]
#[test]
fn https_proxy_config_rejects_native_tls_backend() {
    let result = builder()
        .tls_backend_native()
        .https_proxy_tls_config(|_| panic!("must fail at build time"))
        .build();
    assert!(result.unwrap_err().is_builder());
}

#[cfg(feature = "blocking")]
#[test]
fn blocking_https_proxy_config() {
    reqwest::blocking::Client::builder()
        .tls_backend_rustls()
        .https_proxy_tls_config(|_| Err("not connecting".into()))
        .build()
        .unwrap();
}
