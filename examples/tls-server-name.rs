//! HTTPS GET client with custome TLS server name based on hyper-tls
//!
//! First parameter is the URL to GET.
//! Second parameter is the TLS server name to negociate.
#![deny(warnings)]

// This is using the `tokio` runtime. You'll need the following dependency:
//
// `tokio = { version = "1", features = ["full"] }`
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let mut args = std::env::args().skip(1);

    let (Some(url), Some(server_name)) = (args.next(), args.next()) else {
        println!("Usage: client <url> <server_name>");
        return Ok(());
    };

    eprintln!("Building client");

    let client = reqwest::Client::builder()
        .tls_server_name(Some(server_name.into()))
        .build()
        .expect("client should build");

    eprintln!("Fetching {url:?}...");

    let res = client.get(url).send().await?;

    eprintln!("Response: {:?} {}", res.version(), res.status());
    eprintln!("Headers: {:#?}\n", res.headers());

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
