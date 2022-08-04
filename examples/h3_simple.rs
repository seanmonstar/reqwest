#![deny(warnings)]

use http::Version;
use reqwest::{Client, IntoUrl, Response};

async fn get<T: IntoUrl + Clone>(url: T) -> reqwest::Result<Response> {
    let client = Client::builder()
        .http3_prior_knowledge()
        .build()?;

    client.get(url.clone())
        .version(Version::HTTP_3)
        .send()
        .await.unwrap();
    client.get(url)
        .version(Version::HTTP_3)
        .send()
        .await
}


// This is using the `tokio` runtime. You'll need the following dependency:
//
// `tokio = { version = "1", features = ["full"] }`
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    // Some simple CLI args requirements...
    let url = match std::env::args().nth(1) {
        Some(url) => url,
        None => {
            println!("No CLI URL provided, using default.");
            "https://hyper.rs".into()
        }
    };

    eprintln!("Fetching {:?}...", url);

    let res = get(url).await?;

    eprintln!("Response: {:?} {}", res.version(), res.status());
    eprintln!("Headers: {:#?}\n", res.headers());

    let body = res.text().await?;

    println!("{}", body);

    Ok(())
}

// The [cfg(not(target_arch = "wasm32"))] above prevent building the tokio::main function
// for wasm32 target, because tokio isn't compatible with wasm32.
// If you aren't building for wasm32, you don't need that line.
// The two lines below avoid the "'main' function not found" error when building for wasm32 target.
#[cfg(target_arch = "wasm32")]
fn main() {}
