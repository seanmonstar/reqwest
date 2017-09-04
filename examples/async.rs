#![allow(warnings)] // remove when error_chain is fixed

extern crate futures;
extern crate reqwest;
extern crate tokio_core;
#[macro_use]
extern crate error_chain;

use std::mem;
use std::io::{self, Cursor};
use futures::{Future, Stream};
use reqwest::unstable::async::{Client, Decoder};

error_chain! {
    foreign_links {
        ReqError(reqwest::Error);
        IoError(io::Error);
    }
}

fn run() -> Result<()> {
    let mut core = tokio_core::reactor::Core::new()?;
    let client = Client::new(&core.handle());

    let work = client.get("https://hyper.rs")
        .send()
        .map_err(|e| Error::from(e))
        .and_then(|mut res| {
            println!("{}", res.status());

            let body = mem::replace(res.body_mut(), Decoder::empty());
            body.concat2().map_err(Into::into)
        })
        .and_then(|body| {
            let mut body = Cursor::new(body);
            io::copy(&mut body, &mut io::stdout()).map_err(Into::into)
        });

    core.run(work)?;
    Ok(())
}

quick_main!(run);
