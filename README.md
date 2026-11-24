# reqwest

[![crates.io](https://img.shields.io/crates/v/reqwest.svg)](https://crates.io/crates/reqwest)
[![Documentation](https://docs.rs/reqwest/badge.svg)](https://docs.rs/reqwest)
[![MIT/Apache-2 licensed](https://img.shields.io/crates/l/reqwest.svg)](./LICENSE-APACHE)
[![CI](https://github.com/seanmonstar/reqwest/actions/workflows/ci.yml/badge.svg)](https://github.com/seanmonstar/reqwest/actions/workflows/ci.yml)

An ergonomic, batteries-included HTTP Client for Rust.

- Async and blocking `Client`s
- Plain bodies, JSON, urlencoded, multipart
- Customizable redirect policy
- HTTP Proxies
- HTTPS via rustls (or optionally, system-native TLS)
- Cookie Store
- WASM


## Example

This asynchronous example uses [Tokio](https://tokio.rs) and enables some
optional features, so your `Cargo.toml` could look like this:

```toml
[dependencies]
reqwest = { version = "0.13", features = ["json"] }
tokio = { version = "1", features = ["full"] }
```

And then the code:

```rust,no_run
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp = reqwest::get("https://httpbin.org/ip")
        .await?
        .json::<HashMap<String, String>>()
        .await?;
    println!("{resp:#?}");
    Ok(())
}
```

## Commercial Support

For private advice, support, reviews, access to the maintainer, and the like, reach out for [commercial support][sponsor].

## Requirements

By default, Reqwest uses [rustls](https://github.com/rustls/rustls), but when the `native-tls` feature is enabled
it will use the operating system TLS framework if available, meaning Windows and macOS.
On Linux, it will use the available OpenSSL (see https://docs.rs/openssl for supported versions and more details)
or fail to build if not found. Alternatively you can enable the `native-tls-vendored` feature to compile a copy of OpenSSL.

### Using `rustls-no-provider`

The `rustls` feature (the default) ships with the `aws-lc-rs` crypto provider. To use a different
provider—for example [`ring`](https://crates.io/crates/ring)—enable `rustls-no-provider` instead
and install the provider yourself before creating any clients:

```toml
reqwest = { version = "0.13", default-features = false, features = ["rustls-no-provider"] }
rustls = { version = "0.23", features = ["ring"] }
```

```rust,no_run
#[tokio::main]
async fn main() {
    // Required when using `rustls-no-provider`; omitting this panics at Client construction.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let client = reqwest::Client::new();
    // ...
}
```

See the [`tls` module docs](https://docs.rs/reqwest/latest/reqwest/tls/index.html) for more detail on TLS backends.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.

## Sponsors

Support this project by becoming a [sponsor][].

[sponsor]: https://seanmonstar.com/sponsor
