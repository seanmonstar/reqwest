extern crate reqwest;
extern crate env_logger;

use std::io::Read;

fn main() {
    env_logger::init().unwrap();

    println!("GET https://www.rust-lang.org");

    let mut res = reqwest::get("https://www.rust-lang.org").unwrap();

    println!("Status: {}", res.status());
    println!("Headers:\n{}", res.headers());

    ::std::io::copy(&mut res, &mut ::std::io::stdout()).unwrap();


    let url = "https://doc.rust-lang.org/stable/std/io/trait.Read.html";
    println!("\n\n\nGET {}", url);

    let mut response = reqwest::get(url).unwrap();

    println!("Status: {}", res.status());
    println!("Headers:\n{}", res.headers());

    let mut result_string = String::new();
    response.read_to_string(&mut result_string).unwrap();

    println!("Read {} bytes into a string.", result_string.len());

    println!("\n\nDone.");
}
