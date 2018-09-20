#![deny(warnings)]

extern crate futures;
extern crate reqwest;
extern crate tokio;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_json;

use futures::Future;
use reqwest::async::{Client, Response};

#[derive(Deserialize, Debug)]
struct Slideshow {
    title: String,
    author: String,
}

#[derive(Deserialize, Debug)]
struct SlideshowContainer {
    slideshow: Slideshow,
}

fn fetch() -> impl Future<Item=(), Error=()> {
    let client = Client::new();

    let json = |mut res : Response | {
        res.json::<SlideshowContainer>()
    };

    let request1 =
        client
            .get("https://httpbin.org/json")
            .send()
            .and_then(json);

    let request2 =
        client
            .get("https://httpbin.org/json")
            .send()
            .and_then(json);

    request1.join(request2)
        .map(|(res1, res2)|{
            println!("{:?}", res1);
            println!("{:?}", res2);
        })
        .map_err(|err| {
            println!("stdout error: {}", err);
        })
}

fn main() {
    tokio::run(fetch());
}
