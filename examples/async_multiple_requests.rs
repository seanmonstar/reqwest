#![deny(warnings)]

use reqwest::r#async::{Client, Response};
use serde::Deserialize;
use std::future::Future;

#[derive(Deserialize, Debug)]
struct Slideshow {
    title: String,
    author: String,
}

#[derive(Deserialize, Debug)]
struct SlideshowContainer {
    slideshow: Slideshow,
}

async fn into_json<F>(f: F) -> Result<SlideshowContainer, reqwest::Error>
where
    F: Future<Output = Result<Response, reqwest::Error>>,
{
    let mut resp = f.await?;
    resp.json::<SlideshowContainer>().await
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let client = Client::new();

    let request1 = client.get("https://httpbin.org/json").send();

    let request2 = client.get("https://httpbin.org/json").send();

    let (try_json1, try_json2) =
        futures::future::join(into_json(request1), into_json(request2)).await;

    println!("{:?}", try_json1?);
    println!("{:?}", try_json2?);

    Ok(())
}
