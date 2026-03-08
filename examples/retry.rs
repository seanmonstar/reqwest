#![deny(warnings)]

// Demonstrates how to configure request retries using reqwest's retry module.
//
// This is using the `tokio` runtime. You'll need the following dependency:
// `tokio = { version = "1", features = ["full"] }`

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    env_logger::init();

    // Configure retries scoped to a specific host.
    //
    // Only GET requests that receive a server error (5xx) will be retried,
    // up to 2 times per request. A retry budget (default 20% extra load)
    // prevents retry storms.
    let retries = reqwest::retry::for_host("httpbin.org")
        .max_retries_per_request(2)
        .classify_fn(|req_rep| match (req_rep.method(), req_rep.status()) {
            (&http::Method::GET, Some(status)) if status.is_server_error() => {
                eprintln!("Server error {status}, will retry...");
                req_rep.retryable()
            }
            _ => req_rep.success(),
        });

    let client = reqwest::Client::builder().retry(retries).build()?;

    let url = if let Some(url) = std::env::args().nth(1) {
        url
    } else {
        println!("No CLI URL provided, using default.");
        "https://httpbin.org/get".into()
    };

    eprintln!("Fetching {url:?}...");

    let res = client.get(&url).send().await?;

    eprintln!("Response: {:?} {}", res.version(), res.status());

    let body = res.text().await?;
    println!("{body}");

    Ok(())
}

// The [cfg(not(target_arch = "wasm32"))] above prevent building the tokio::main function
// for wasm32 target, because tokio isn't compatible with wasm32.
// If you aren't building for wasm32, you don't need that line.
// The two lines below avoid the "'main' function not found" error when building for wasm32 target.
#[cfg(target_arch = "wasm32")]
fn main() {}
