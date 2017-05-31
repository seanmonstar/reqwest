//! `cargo run --example response_json`
extern crate reqwest;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate error_chain;

error_chain! {
    foreign_links {
        ReqError(reqwest::Error);
    }
}

#[derive(Debug, Deserialize)]
struct Response {
    origin: String,
}

fn run() -> Result<()> {
    let mut res = reqwest::get("https://httpbin.org/ip")?;
    let json = res.json::<Response>()?;
    println!("JSON: {:?}", json);
    Ok(())
}

quick_main!(run);
