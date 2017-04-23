// cargo run --example response_json
extern crate reqwest;

#[macro_use]
extern crate serde_derive;

#[derive(Debug, Deserialize)]
struct Response {
    origin: String,
}

fn main() {
    let mut res = reqwest::get("https://httpbin.org/ip").unwrap();
    println!("JSON: {:?}", res.json::<Response>());
}
