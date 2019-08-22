//! `cargo run --example simple`
#![deny(warnings)]
use failure::Fail;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Io Error")]
    Io(#[fail(cause)] std::io::Error),
    #[fail(display = "Reqwest error")]
    Reqwest(#[fail(cause)] reqwest::Error),
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();

    println!("GET https://www.rust-lang.org");

    let res = reqwest::get("https://www.rust-lang.org/").map_err(Error::Reqwest)?;

    println!("Status: {}", res.status());
    println!("Headers:\n{:?}", res.headers());

    // copy the response body directly to stdout
    //TODO: Read/Write
//    std::io::copy(&mut res, &mut std::io::stdout()).map_err(Error::Io)?;

    println!("\n\nDone.");
    Ok(())
}
