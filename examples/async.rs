#![deny(warnings)]
extern crate futures;
extern crate reqwest;
extern crate tokio_core;
extern crate hyper;

use futures::Future;

fn main() {
    let mut core = tokio_core::reactor::Core::new().unwrap();

    let client = reqwest::unstable::async::ClientBuilder::new().unwrap()
        .build(&core.handle()).unwrap();

    let work = client.get("https://hyper.rs").unwrap().send().and_then(|mut res| {
        let status = res.status();
        let headers = res.headers().clone();
        println!("status: {:?}", status);
        println!("headers: {:?}", headers);
        res.body_resolved()
    })
        .map(|resp| {
            println!("resp: {:?}", String::from_utf8_lossy(&*resp));
        });

    core.run(work).unwrap();
}
