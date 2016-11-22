extern crate reqwest;
extern crate env_logger;
extern crate hyper;

use reqwest::header::{AcceptEncoding, Encoding, qitem};

fn main() {
    env_logger::init().unwrap();

    println!("GET https://www.rust-lang.org");

    let mut client = reqwest::Client::new().unwrap();

    let mut builder = client.get("https://www.google.com")
    	.header(AcceptEncoding(vec![
    		qitem(Encoding::Gzip)
		]))
	;

	println!("{:?}", builder);

	let mut res = builder
    	.send()
    	.unwrap();

    println!("Status: {:?}", res.status());
    println!("Headers:\n{:?}\n\nBody:\n---\n", res.headers());

    ::std::io::copy(&mut res, &mut ::std::io::stdout()).unwrap();

    println!("\n\nDone.");
}
