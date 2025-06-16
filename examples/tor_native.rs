#![deny(warnings)]

// This is using the `tokio` runtime. You'll need the following dependency:
//
// `tokio = { version = "1", features = ["full"] }`
#[cfg(feature = "tor")]
#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    // Note: on macos use `reqwest::ClientBuilder::tor(Default::default()).await.unwrap().use_rustls_tls().build()?;`
    let client = reqwest::Client::tor().await;

    let res = client.get("https://check.torproject.org").send().await?;
    println!("Status: {}", res.status());

    let text = res.text().await?;
    let is_tor = text.contains("Congratulations. This browser is configured to use Tor.");
    println!("Is Tor: {is_tor}");
    assert!(is_tor);

    Ok(())
}

#[cfg(not(feature = "tor"))]

fn main() {}
