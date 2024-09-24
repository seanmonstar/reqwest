# reqwest

[![crates.io](https://img.shields.io/crates/v/reqwest.svg)](https://crates.io/crates/reqwest)
[![Documentation](https://docs.rs/reqwest/badge.svg)](https://docs.rs/reqwest)
[![MIT/Apache-2 licensed](https://img.shields.io/crates/l/reqwest.svg)](./LICENSE-APACHE)
[![CI](https://github.com/seanmonstar/reqwest/workflows/CI/badge.svg)](https://github.com/seanmonstar/reqwest/actions?query=workflow%3ACI)

## Table of Contents
- [Introduction](#introduction)
- [Installation](#installation)
- [Example](#example)
- [Usage](#usage)
  - [Sending a POST Request with JSON](#sending-a-post-request-with-json)
  - [Handling Custom Headers](#handling-custom-headers)
  - [Managing Timeouts](#managing-timeouts)
- [Commercial Support](#commercial-support)
- [Requirements](#requirements)
- [License](#license)
- [Contribution](#contribution)
- [Sponsors](#sponsors)

## Introduction
An ergonomic, batteries-included HTTP Client for Rust. It provides a rich set of features, including:

- Async and blocking `Client`s
- Plain bodies, JSON, urlencoded, multipart
- Customizable redirect policy
- HTTP Proxies
- HTTPS via system-native TLS (or optionally, rustls)
- Cookie Store
- WASM

## Installation
To use `reqwest` in your Rust project, add the following to your `Cargo.toml` file:

```toml
[dependencies]
reqwest = "0.12"
```

For additional features like JSON and cookie session support, you can include:
```toml
[dependencies]
reqwest = { version = "0.12", features = ["json", "cookies"] }
```

Then run `cargo build` to install the library and its dependencies.

## Example

This asynchronous example uses [Tokio](https://tokio.rs) and enables some
optional features, so your `Cargo.toml` could look like this:

```toml
[dependencies]
reqwest = { version = "0.12", features = ["json"] }
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

## Usage
### Sending a POST Request with JSON
```rust,no_run
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let mut map = HashMap::new();
    map.insert("key", "value");

    let response = client.post("https://httpbin.org/post")
        .json(&map)
        .send()
        .await?;

    println!("Response: {:?}", response.text().await?);
    Ok(())
}
```
In this example, a new `Client` is created, and a POST request is sent to the `httpbin.org` service with a JSON body containing a key-value pair. The server response is printed. This demonstrates how to use the `json` method to serialize a Rust data structure into JSON and include it in the request body.

### Handling Custom Headers
```rust,no_run
use reqwest::header;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client.get("https://httpbin.org/headers")
        .header(header::USER_AGENT, "reqwest")
        .send()
        .await?
        .text()
        .await?;
    println!("{response}");
    Ok(())
}
```
In this example, a `USER_AGENT` header is added to a GET request sent to `httpbin.org/headers`. The response, which includes all headers sent by the client, is printed. This demonstrates how to use the `header` method on the client to add custom headers to a request.

### Managing Timeouts
```rust,no_run
use std::time::Duration;
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let response = client.get("https://httpbin.org/delay/5")
        .send()
        .await?
        .text()
        .await?;
    println!("{response}");
    Ok(())
}
```
In this example, a client is created with a timeout of 10 seconds. A GET request is sent to an endpoint that delays the response by 5 seconds, and the response is printed at the end. This demonstrates how to set a timeout for all requests made with the client using the `timeout` method, which ensures any request taking longer than the specified duration will be aborted.

## Commercial Support

For private advice, support, reviews, access to the maintainer, and the like, reach out for [commercial support][sponsor].

## Requirements

On Linux:

- OpenSSL with headers. See https://docs.rs/openssl for supported versions
  and more details. Alternatively you can enable the `native-tls-vendored`
  feature to compile a copy of OpenSSL.

On Windows and macOS:

- Nothing.

Reqwest uses [rust-native-tls](https://github.com/sfackler/rust-native-tls),
which will use the operating system TLS framework if available, meaning Windows
and macOS. On Linux, it will use the available OpenSSL or fail to build if
not found.


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
