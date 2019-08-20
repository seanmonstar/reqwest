#![deny(warnings)]
use std::mem;
use std::io::{self, Cursor};
use futures::{Future, Stream, TryStreamExt};
use reqwest::r#async::{Client, Decoder};

#[tokio::main]
fn main() -> Result<(), reqwest::Error> {
    let mut res = Client::new()
        .get("https://hyper.rs")
        .send()
        .await?;

    println!("{}", res.status());

    let body = mem::replace(res.body_mut(), Decoder::empty());
    let body: Result<_, _> = body.try_concat().await;

    let mut body = Cursor::new(body?);
    io::copy(&mut body, &mut io::stdout())
        .map_err(|err| {
            println!("stdout error: {}", err);
        })?;

    Ok(())
}
