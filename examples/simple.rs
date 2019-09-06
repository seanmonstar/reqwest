#![deny(warnings)]

//! `cargo run --example simple`

extern crate reqwest;
extern crate env_logger;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("GET https://www.rust-lang.org");

    let mut res = reqwest::get("https://www.rust-lang.org/")?;

    println!("Status: {}", res.status());
    println!("Headers:\n{:?}", res.headers());

    // copy the response body directly to stdout
    std::io::copy(&mut res, &mut std::io::stdout())?;

    println!("\n\nDone.");
    Ok(())
}
