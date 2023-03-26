#![deny(warnings)]

use std::net::SocketAddr;
use futures_core::future::BoxFuture;

// This is using the `tokio` runtime. You'll need the following dependency:
//
// `tokio = { version = "1", features = ["full"] }`
#[cfg(feature = "http3-provider")]
#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    use http::Version;
    use reqwest::{Client, IntoUrl, Response};

    async fn get<T: IntoUrl + Clone>(url: T) -> reqwest::Result<Response> {
        Client::builder()
            .http3_prior_knowledge()
            .http3_connection_provider(Box::new(MyHttp3Provider{}))
            .build()?
            .get(url)
            .version(Version::HTTP_3)
            .send()
            .await
    }

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
#[cfg(any(target_arch = "wasm32", not(feature = "http3-provider")))]
fn main() {}

struct MyHttp3Provider {

}

impl crate::async_impl::h3_client::connect::H3ConnectionProvider for MyHttp3Provider {
    type C = ();
    type T = ();

    fn poll_connect(
        &self,
        addr: SocketAddr,
        server_name: &str,
    ) -> BoxFuture<Result<Self::C, BoxError>> {
        todo!()
    }
}