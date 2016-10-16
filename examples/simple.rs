extern crate reqwest;
extern crate env_logger;

fn main() {
    env_logger::init().unwrap();

    println!("GET https://www.rust-lang.org");

    let mut res = reqwest::get("http://www.rust-lang.org").unwrap();

    println!("Status: {}", res.status());
    println!("Headers:\n{}", res.headers());

    ::std::io::copy(&mut res, &mut ::std::io::stdout()).unwrap();

    println!("\n\nDone.");
}
