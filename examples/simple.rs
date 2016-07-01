extern crate request;
extern crate env_logger;

fn main() {
    env_logger::init().unwrap();

    let mut res = request::get("https://rust-lang.org").unwrap();

    println!("Status: {}", res.status());
    println!("Headers:\n{}", res.headers());

    ::std::io::copy(&mut res, &mut ::std::io::stdout()).unwrap();

    println!("\n\nDone.");
}
