#![allow(warnings)] // remove when error_chain is fixed

//! `cargo run --example simple`

extern crate reqwest;
extern crate env_logger;
#[macro_use]
extern crate error_chain;

error_chain! {
    foreign_links {
        ReqError(reqwest::Error);
        IoError(std::io::Error);
    }
}

fn run() -> Result<()> {
    env_logger::init().expect("Failed to initialize logger");

    println!("GET https://www.rust-lang.org");

    let mut res = reqwest::get("https://www.rust-lang.org/en-US/")?;

    println!("Status: {}", res.status());
    println!("Headers:\n{}", res.headers());

    // copy the response body directly to stdout
    let _ = std::io::copy(&mut res, &mut std::io::stdout())?;

    println!("\n\nDone.");
    Ok(())
}

quick_main!(run);
