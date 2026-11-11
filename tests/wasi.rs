#![cfg(not(target_arch = "wasm32"))]
mod support;

use std::convert::Infallible;
use std::io;
use std::process::Command;
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use http::{Request, Response};
use http_body_util::BodyExt;
use serde_json::{json, Map, Value};
use support::server;

const CHECKS: &[&str] = &[
    "get_json",
    "post_bytes",
    "post_empty",
    "post_json",
    "multipart",
    "headers",
    "url_credentials",
    "chunks",
    "bytes_stream",
    "large_response",
    "request_timeout",
    "client_timeout",
    "body_timeout",
    "refused",
    "status_error",
    "aborted_body",
    "concurrent",
    "redirect_returned",
    "dropped_request",
];

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn header(req: &Request<hyper::body::Incoming>, name: &str) -> Value {
    req.headers()
        .get(name)
        .map_or(Value::Null, |v| json!(v.to_str().unwrap()))
}

async fn route(req: Request<hyper::body::Incoming>) -> Response<reqwest::Body> {
    let path = req.uri().path().to_string();
    match path.as_str() {
        "/json" => Response::new(r#"{"hello":"world"}"#.into()),
        "/echo" => {
            let content_length = header(&req, "content-length");
            let content_type = header(&req, "content-type");
            let body = req.into_body().collect().await.unwrap().to_bytes();
            let reply = json!({
                "len": body.len(),
                "sum": body.iter().map(|b| u64::from(*b)).sum::<u64>(),
                "content_length": content_length,
                "content_type": content_type,
                "text": String::from_utf8_lossy(&body),
            });
            Response::new(reply.to_string().into())
        }
        "/headers" => {
            let mut sent = Map::new();
            for (name, value) in req.headers() {
                sent.entry(name.as_str())
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .unwrap()
                    .push(json!(value.to_str().unwrap()));
            }
            Response::builder()
                .header("x-multi", "a")
                .header("x-multi", "b")
                .header("set-cookie", "c=1")
                .header("set-cookie", "d=2")
                .body(Value::Object(sent).to_string().into())
                .unwrap()
        }
        "/chunks" => {
            let chunks = stream::unfold(0, |i| async move {
                if i == 4 {
                    return None;
                }
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Some((Ok::<_, Infallible>(Bytes::from(i.to_string())), i + 1))
            });
            Response::new(reqwest::Body::wrap_stream(chunks))
        }
        "/large" => Response::new(pattern(8 << 20).into()),
        "/slow" => {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            Response::new("slow".into())
        }
        "/stall" => {
            let body = stream::unfold(0, |step| async move {
                match step {
                    0 => Some((Ok::<_, Infallible>(Bytes::from("first")), 1)),
                    1 => {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        Some((Ok(Bytes::from("late")), 2))
                    }
                    _ => None,
                }
            });
            Response::new(reqwest::Body::wrap_stream(body))
        }
        "/abort" => {
            let body = stream::unfold(0, |step| async move {
                match step {
                    0 => Some((Ok(Bytes::from("0123456789")), 1)),
                    1 => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Some((Err(io::Error::other("aborted")), 2))
                    }
                    _ => None,
                }
            });
            Response::builder()
                .header("content-length", "1000")
                .body(reqwest::Body::wrap_stream(body))
                .unwrap()
        }
        "/redirect" => Response::builder()
            .status(302)
            .header("location", "/json")
            .body("".into())
            .unwrap(),
        "/hang" => {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Response::new("".into())
        }
        "/status/404" => Response::builder().status(404).body("".into()).unwrap(),
        _ => Response::builder().status(500).body("".into()).unwrap(),
    }
}

#[test]
#[ignore]
fn component() {
    let component = std::env::var("REQWEST_WASI_COMPONENT").expect("REQWEST_WASI_COMPONENT");
    let wasmtime = std::env::var("WASMTIME").unwrap_or_else(|_| "wasmtime".into());
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let server = server::http(route);

    let output = Command::new(wasmtime)
        .args([
            "run",
            "-S",
            "p3",
            "-S",
            "http",
            "-W",
            "component-model-async",
        ])
        .arg(component)
        .arg(format!("http://{}", server.addr()))
        .arg(closed.to_string())
        .args(CHECKS)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
}
