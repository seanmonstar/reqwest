use std::future::{poll_fn, Future};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use wasip3::clocks::monotonic_clock;

type Check = Result<(), String>;

fn now_ms() -> u64 {
    monotonic_clock::now() / 1_000_000
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn sum(bytes: &[u8]) -> u64 {
    bytes.iter().map(|b| u64::from(*b)).sum()
}

fn ensure(condition: bool, what: impl std::fmt::Display) -> Check {
    if condition {
        Ok(())
    } else {
        Err(what.to_string())
    }
}

fn failed<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
}

async fn within<F: Future>(limit_ms: u64, future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    let mut timer = pin!(monotonic_clock::wait_until(
        monotonic_clock::now() + limit_ms * 1_000_000
    ));
    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Some(output));
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

async fn get_json(base: &str) -> Check {
    let response = reqwest::get(format!("{base}/json")).await.map_err(failed)?;
    ensure(response.status() == 200, "status")?;
    ensure(response.url().path() == "/json", "url")?;
    let value: Value = response.json().await.map_err(failed)?;
    ensure(value == json!({"hello": "world"}), value)
}

async fn post_bytes(base: &str) -> Check {
    let body = pattern(300_000);
    let sum = sum(&body);
    let reply: Value = reqwest::Client::new()
        .post(format!("{base}/echo"))
        .body(body)
        .send()
        .await
        .map_err(failed)?
        .json()
        .await
        .map_err(failed)?;
    ensure(reply["len"] == 300_000 && reply["sum"] == sum, &reply)?;
    ensure(reply["content_length"] == "300000", &reply)
}

async fn post_empty(base: &str) -> Check {
    let reply: Value = reqwest::Client::new()
        .post(format!("{base}/echo"))
        .send()
        .await
        .map_err(failed)?
        .json()
        .await
        .map_err(failed)?;
    ensure(reply["len"] == 0 && reply["content_length"] == "0", &reply)
}

async fn post_json(base: &str) -> Check {
    let reply: Value = reqwest::Client::new()
        .post(format!("{base}/echo"))
        .json(&json!({"a": [1, 2]}))
        .send()
        .await
        .map_err(failed)?
        .json()
        .await
        .map_err(failed)?;
    ensure(reply["content_type"] == "application/json", &reply)?;
    ensure(reply["text"] == r#"{"a":[1,2]}"#, &reply)
}

async fn multipart(base: &str) -> Check {
    let form = reqwest::multipart::Form::new().text("title", "hello").part(
        "file",
        reqwest::multipart::Part::bytes(vec![0, 1, 255])
            .file_name("a\"b.bin")
            .mime_str("application/octet-stream")
            .map_err(failed)?,
    );
    let reply: Value = reqwest::Client::new()
        .post(format!("{base}/echo"))
        .multipart(form)
        .send()
        .await
        .map_err(failed)?
        .json()
        .await
        .map_err(failed)?;
    let content_type = reply["content_type"].as_str().unwrap_or_default();
    let text = reply["text"].as_str().unwrap_or_default();
    ensure(
        content_type.starts_with("multipart/form-data; boundary="),
        &reply,
    )?;
    ensure(
        text.contains("name=\"title\"\r\n\r\nhello\r\n")
            && text.contains("filename=\"a%22b.bin\"")
            && text.contains("Content-Type: application/octet-stream"),
        &reply,
    )?;
    ensure(
        reply["len"].as_u64()
            == reply["content_length"]
                .as_str()
                .and_then(|l| l.parse().ok()),
        &reply,
    )
}

async fn headers(base: &str) -> Check {
    let client = reqwest::Client::builder()
        .user_agent("wasi-test")
        .build()
        .map_err(failed)?;
    let response = client
        .get(format!("{base}/headers"))
        .header("x-list", "a")
        .header("x-list", "b")
        .bearer_auth("token")
        .send()
        .await
        .map_err(failed)?;
    let values = |name: &str| -> Vec<String> {
        response
            .headers()
            .get_all(name)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect()
    };
    ensure(
        values("x-multi") == ["a", "b"],
        format!("{:?}", values("x-multi")),
    )?;
    ensure(
        values("set-cookie") == ["c=1", "d=2"],
        format!("{:?}", values("set-cookie")),
    )?;
    let sent: Value = response.json().await.map_err(failed)?;
    ensure(sent["x-list"] == json!(["a", "b"]), &sent)?;
    ensure(sent["user-agent"] == json!(["wasi-test"]), &sent)?;
    ensure(sent["authorization"] == json!(["Bearer token"]), &sent)?;
    ensure(sent["accept"] == json!(["*/*"]), &sent)
}

async fn url_credentials(base: &str) -> Check {
    let url = base.replacen("http://", "http://us%40er:pa:ss@", 1);
    let response = reqwest::get(format!("{url}/headers"))
        .await
        .map_err(failed)?;
    ensure(
        response.url().username().is_empty() && response.url().password().is_none(),
        response.url(),
    )?;
    let sent: Value = response.json().await.map_err(failed)?;
    ensure(
        sent["authorization"] == json!(["Basic dXNAZXI6cGE6c3M="]),
        &sent,
    )?;
    ensure(
        sent["host"] == json!([base.trim_start_matches("http://")]),
        &sent,
    )
}

async fn chunks(base: &str) -> Check {
    let mut response = reqwest::get(format!("{base}/chunks"))
        .await
        .map_err(failed)?;
    let start = now_ms();
    let mut seen = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        seen.push((now_ms() - start, chunk));
    }
    ensure(seen.len() == 4, format!("{} chunks", seen.len()))?;
    ensure(
        seen[3].0 - seen[0].0 >= 600,
        format!("chunks arrived at once: {seen:?}"),
    )?;
    ensure(
        seen.iter()
            .map(|(_, c)| c.as_ref())
            .collect::<Vec<_>>()
            .concat()
            == b"0123",
        "chunks",
    )
}

async fn bytes_stream(base: &str) -> Check {
    let response = reqwest::get(format!("{base}/chunks"))
        .await
        .map_err(failed)?;
    let mut stream = response.bytes_stream();
    let mut total = 0;
    while let Some(chunk) = stream.next().await {
        total += chunk.map_err(failed)?.len();
    }
    ensure(total == 4, total)
}

async fn large_response(base: &str) -> Check {
    let response = reqwest::get(format!("{base}/large"))
        .await
        .map_err(failed)?;
    ensure(response.content_length() == Some(8 << 20), "content length")?;
    let bytes = response.bytes().await.map_err(failed)?;
    ensure(
        bytes.len() == 8 << 20 && sum(&bytes) == sum(&pattern(8 << 20)),
        "bytes",
    )
}

async fn request_timeout(base: &str) -> Check {
    let start = now_ms();
    let error = reqwest::Client::new()
        .get(format!("{base}/slow"))
        .timeout(Duration::from_millis(300))
        .send()
        .await
        .err()
        .ok_or("no error")?;
    ensure(
        error.is_timeout() && error.is_request(),
        format!("{error:?}"),
    )?;
    ensure(now_ms() - start < 2_000, "late")
}

async fn client_timeout(base: &str) -> Check {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(300))
        .build()
        .map_err(failed)?;
    let error = client
        .get(format!("{base}/slow"))
        .send()
        .await
        .err()
        .ok_or("no error")?;
    ensure(error.is_timeout(), format!("{error:?}"))?;
    let response = client
        .get(format!("{base}/slow"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(failed)?;
    ensure(response.status() == 200, "the request's own timeout wins")
}

async fn body_timeout(base: &str) -> Check {
    let response = reqwest::Client::new()
        .get(format!("{base}/stall"))
        .timeout(Duration::from_millis(500))
        .send()
        .await
        .map_err(failed)?;
    let error = response.bytes().await.err().ok_or("no error")?;
    ensure(error.is_timeout() && error.is_body(), format!("{error:?}"))
}

async fn refused(closed: &str) -> Check {
    let error = reqwest::get(format!("http://{closed}/"))
        .await
        .err()
        .ok_or("no error")?;
    ensure(
        error.is_request() && !error.is_timeout(),
        format!("{error:?}"),
    )
}

async fn status_error(base: &str) -> Check {
    let response = reqwest::get(format!("{base}/status/404"))
        .await
        .map_err(failed)?;
    ensure(response.status() == 404, "status")?;
    let error = response.error_for_status().err().ok_or("no error")?;
    ensure(
        error.is_status() && error.status() == Some(reqwest::StatusCode::NOT_FOUND),
        format!("{error:?}"),
    )
}

async fn aborted_body(base: &str) -> Check {
    let response = reqwest::get(format!("{base}/abort"))
        .await
        .map_err(failed)?;
    let error = response.bytes().await.err().ok_or("no error")?;
    ensure(error.is_body(), format!("{error:?}"))
}

async fn concurrent(base: &str) -> Check {
    let client = reqwest::Client::new();
    let start = now_ms();
    let results =
        futures_util::future::join_all((0..16).map(|_| client.get(format!("{base}/slow")).send()))
            .await;
    ensure(results.iter().all(|r| r.is_ok()), "a request failed")?;
    ensure(
        now_ms() - start < 8_000,
        "the requests ran one after another",
    )
}

async fn redirect_returned(base: &str) -> Check {
    let response = reqwest::get(format!("{base}/redirect"))
        .await
        .map_err(failed)?;
    ensure(
        response.status() == 302 && response.headers()["location"] == "/json",
        "redirect",
    )
}

async fn dropped_request(base: &str) -> Check {
    let client = reqwest::Client::new();
    ensure(
        within(200, client.get(format!("{base}/hang")).send())
            .await
            .is_none(),
        "answered",
    )
}

async fn run_check(name: &str, base: &str, closed: &str) -> Check {
    match name {
        "get_json" => get_json(base).await,
        "post_bytes" => post_bytes(base).await,
        "post_empty" => post_empty(base).await,
        "post_json" => post_json(base).await,
        "multipart" => multipart(base).await,
        "headers" => headers(base).await,
        "url_credentials" => url_credentials(base).await,
        "chunks" => chunks(base).await,
        "bytes_stream" => bytes_stream(base).await,
        "large_response" => large_response(base).await,
        "request_timeout" => request_timeout(base).await,
        "client_timeout" => client_timeout(base).await,
        "body_timeout" => body_timeout(base).await,
        "refused" => refused(closed).await,
        "status_error" => status_error(base).await,
        "aborted_body" => aborted_body(base).await,
        "concurrent" => concurrent(base).await,
        "redirect_returned" => redirect_returned(base).await,
        "dropped_request" => dropped_request(base).await,
        other => Err(format!("unknown check {other}")),
    }
}

struct Component;

impl wasip3::exports::cli::run::Guest for Component {
    async fn run() -> Result<(), ()> {
        let mut args = wasip3::cli::environment::get_arguments()
            .into_iter()
            .skip(1);
        let base = args.next().ok_or(())?;
        let closed = args.next().ok_or(())?;
        let mut failures = 0;
        for name in args {
            match within(30_000, run_check(&name, &base, &closed)).await {
                Some(Ok(())) => println!("ok {name}"),
                Some(Err(reason)) => {
                    failures += 1;
                    println!("FAILED {name}: {reason}");
                }
                None => {
                    failures += 1;
                    println!("FAILED {name}: no answer");
                }
            }
        }
        if failures == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

wasip3::cli::command::export!(Component);
