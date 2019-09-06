#![deny(warnings)]

use reqwest::r#async::Client;

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let mut res = Client::new().get("https://hyper.rs").send().await?;

    println!("Status: {}", res.status());

    let body = res.text().await?;

    println!("Body:\n\n{}", body);

    Ok(())
}
