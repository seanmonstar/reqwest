# reqwest

[![Travis CI Status](https://travis-ci.org/seanmonstar/reqwest.svg?branch=master)](https://travis-ci.org/seanmonstar/reqwest)
[![Appveyor CI Status](https://ci.appveyor.com/api/projects/status/9ol6jcamwdcxq9gr?svg=true)](https://ci.appveyor.com/project/seanmonstar/reqwest)
[![crates.io](https://img.shields.io/crates/v/reqwest.svg)](https://crates.io/crates/reqwest)

An ergonomic HTTP Client for Rust.

Built on top of [hyper](http://hyper.rs), with a batteries-included mindset.

- [Documentation](https://docs.rs/reqwest)

## Requirements

On Linux:

- OpenSSL 1.1 (see https://github.com/sfackler/rust-openssl)

On Windows and macOS:

- Nothing.

Reqwest uses [rust-native-tls](https://github.com/sfackler/rust-native-tls), which will use the operating system TLS framework if available, meaning Windows and macOS. On Linux, it will use OpenSSL 1.1.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
