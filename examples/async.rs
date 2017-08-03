#![deny(warnings)]
extern crate futures;
extern crate reqwest;
extern crate tokio_core;

use futures::Future;

fn main() {
    let mut core = tokio_core::reactor::Core::new().unwrap();
    let client = reqwest::unstable::async::Client::new(&core.handle()).unwrap();

    let work = client.get("https://hyper.rs").unwrap().send().map(|res| {
        println!("{}", res.status());
    });

    core.run(work).unwrap();
}
