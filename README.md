# reqwest

[![crates.io](https://img.shields.io/crates/v/reqwest.svg)](https://crates.io/crates/reqwest)
[![Documentation](https://docs.rs/reqwest/badge.svg)](https://docs.rs/reqwest)
[![MIT/Apache-2 licensed](https://img.shields.io/crates/l/reqwest.svg)](./LICENSE)
[![CI](https://github.com/seanmonstar/reqwest/workflows/CI/badge.svg)](https://github.com/seanmonstar/reqwest/actions?query=workflow%3ACI)

An ergonomic, batteries-included HTTP Client for Rust.

- Plain bodies, JSON, urlencoded, multipart
- Customizable redirect policy
- HTTP Proxies
- HTTPS via system-native TLS (or optionally, rustls)
- Cookie Store
- [Changelog](CHANGELOG.md)

## Example

Async:

```rust,no_run
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp: HashMap<String, String> = reqwest::get("https://httpbin.org/ip")
        .await?
        .json()
        .await?;
    println!("{:#?}", resp);
    Ok(())
}
```

Blocking:

```rust,no_run
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp: HashMap<String, String> = reqwest::blocking::get("https://httpbin.org/ip")?
        .json()?;
    println!("{:#?}", resp);
    Ok(())
}
```

## Requirements

On Linux:

- OpenSSL 1.0.1, 1.0.2, or 1.1.0 with headers (see https://github.com/sfackler/rust-openssl)

On Windows and macOS:

- Nothing.

Reqwest uses [rust-native-tls](https://github.com/sfackler/rust-native-tls), which will use the operating system TLS framework if available, meaning Windows and macOS. On Linux, it will use OpenSSL 1.1.


## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
