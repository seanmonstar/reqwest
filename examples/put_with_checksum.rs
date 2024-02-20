//! `cargo run --example blocking --features=blocking`
#![deny(warnings)]
#![allow(clippy::incompatible_msrv)]

use reqwest::{header::ETAG, Client};
use std::{
    error::Error,
    fmt::LowerHex,
    sync::{Arc, Mutex},
};
use tokio_util::io::{InspectReader, ReaderStream};

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    let file_path = "./test.txt";
    let target_url = "https://example.com/put";

    let client = Client::new();

    let hasher = Digest::new();
    let hasher_rc = Arc::new(Mutex::new(hasher));
    let etag = {
        let hasher_rc2 = hasher_rc.clone();
        let reader = tokio::fs::File::open(&file_path).await?;
        let hashing_reader = InspectReader::new(reader, move |bytes| {
            hasher_rc2.lock().unwrap().update(bytes)
        });
        let stream = ReaderStream::new(hashing_reader);
        let body = reqwest::Body::wrap_stream(stream);
        let req = client.put(target_url).body(body).build()?;
        client
            .execute(req)
            .await?
            .headers()
            .get(ETAG)
            .expect("ETAG header not found")
            .to_str()?
    }
    .to_owned();

    // strip leading and trailing "
    let etag = etag.strip_prefix('"').unwrap_or(&etag);
    let etag = etag.strip_suffix('"').unwrap_or(etag);

    let md5sum = Arc::try_unwrap(hasher_rc)
        .expect("Lock still has multiple owners!.")
        .into_inner()?
        .finalize();

    assert_eq!(format!("{:x}", md5sum), etag, "ETAG not like md5sum!");
    Ok(())
}

// This is modeled after e.g. `md-5`'s md5::Digest and this should be a (working!) drop in.
#[derive(Debug)]
struct Digest {}
impl Digest {
    fn new() -> Self {
        Digest {}
    }

    fn update(&mut self, _data: impl AsRef<[u8]>) {}

    fn finalize(self) -> MyArray {
        MyArray([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    }
}

struct MyArray([u8; 16]);

impl LowerHex for MyArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::Debug;
        self.0.as_slice().fmt(f)
    }
}
