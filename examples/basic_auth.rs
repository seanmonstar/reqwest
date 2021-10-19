
// This is using the `tokio` runtime. You'll need the following dependency:
//
// `tokio = { version = "1", features = ["full"] }`
#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let resp = reqwest::Client::new()
        .get("https://httpbin.org/basic-auth/marvin/42")
        .basic_auth("marvin", Some("42"))
        .send()
        .await?;

    println!("{:#?}", resp.status());
    Ok(())
}
