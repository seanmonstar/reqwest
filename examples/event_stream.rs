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

use reqwest::{
    Client,
    header::{Accept, qitem},
    mime,
};
use std::io::{BufRead, BufReader};


fn run() -> Result<()> {
    env_logger::init();

    println!("GET https://horizon-testnet.stellar.org/transactions");

    let client = Client::new();
    let mut res = client.get("https://horizon-testnet.stellar.org/transactions?order=asc")
        .header(Accept(vec![qitem(mime::TEXT_EVENT_STREAM)]))
        .send()?;

    println!("Status: {}", res.status());
    println!("Headers:\n{}", res.headers());

    // Put the response into a buf reader so we can easily grab lines
    let mut reader = BufReader::new(res);
    loop {
        let mut buf = String::new();

        match reader.read_line(&mut buf) {
            Ok(0) => {
                ::std::thread::sleep(::std::time::Duration::new(1, 0));
                println!("waiting...");
            }
            Ok(_) => print!("{}", buf),
            _ => return break,
        }
    }

    println!("\n\nDone.");
    Ok(())
}

quick_main!(run);
