#![deny(warnings)]

// This is using the `tokio` runtime. You'll need the following dependency:
//
// `tokio = { version = "1", features = ["full"] }`
#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "trust-dns")]
#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    use reqwest::dns::{ResolverConfig, ResolverOpts};
    use std::time::Duration;

    let config = ResolverConfig::default();
    let opts = ResolverOpts {
        negative_max_ttl: Some(Duration::from_secs(60)),
        positive_max_ttl: Some(Duration::from_secs(60 * 60)),
        ..Default::default()
    };
    let client = reqwest::Client::builder()
        .dns_config(config, opts)
        .build()
        .expect("unable to build reqwest client");

    let res = client.get("https://doc.rust-lang.org").send().await?;

    println!("Status: {}", res.status());

    let body = res.text().await?;

    println!("Body:\n\n{}...", &body[..512]);

    Ok(())
}

// The [cfg(not(target_arch = "wasm32"))] above prevent building the tokio::main function
// for wasm32 target, because tokio isn't compatible with wasm32.
// If you aren't building for wasm32, you don't need that line.
// The two lines below avoid the "'main' function not found" error when building for wasm32 target.
#[cfg(any(target_arch = "wasm32", not(feature = "trust-dns")))]
fn main() {
    eprintln!(
        "this example must run with \"--features=trust-dns\" and outside of a wasm32 context"
    );
}
