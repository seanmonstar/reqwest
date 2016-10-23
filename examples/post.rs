//#![feature(proc_macro)]

extern crate reqwest;
extern crate env_logger;
//#[macro_use] extern crate serde_derive;

/*
#[derive(Serialize)]
struct Thingy {
    a: i32,
    b: bool,
    c: String,
}
*/

fn main() {
    env_logger::init().unwrap();

    println!("POST https://httpbin.org/post");

    /*
    let thingy = Thingy {
        a: 5,
        b: true,
        c: String::from("reqwest")
    };
    */

    let client = reqwest::Client::new();
    let mut res = client.post("https://httpbin.org/post")
        .body("foo=bar")
        .send().unwrap();

    println!("Status: {}", res.status());
    println!("Headers:\n{}", res.headers());

    ::std::io::copy(&mut res, &mut ::std::io::stdout()).unwrap();

    println!("\n\nDone.");
}
