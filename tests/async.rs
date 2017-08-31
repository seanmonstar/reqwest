#![cfg(feature="unstable")]

extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate reqwest;
extern crate libflate;

#[macro_use]
mod support;

use std::mem;
use reqwest::unstable::async::{Client, Decoder};
use futures::{Future, Stream};
use tokio_core::reactor::Core;
use std::io::Write;
use std::time::Duration;

#[test]
fn async_test_gzip_response() {
    test_gzip(10_000, 4096);
}

#[test]
fn async_test_gzip_single_byte_chunks() {
    test_gzip(10, 1);
}

fn test_gzip(response_size: usize, chunk_size: usize) {
    let content: String = (0..response_size).into_iter().map(|i| format!("test {}", i)).collect();
    let mut encoder = ::libflate::gzip::Encoder::new(Vec::new()).unwrap();
    match encoder.write(content.as_bytes()) {
        Ok(n) => assert!(n > 0, "Failed to write to encoder."),
        _ => panic!("Failed to gzip encode string."),
    };

    let gzipped_content = encoder.finish().into_result().unwrap();

    let mut response = format!("\
            HTTP/1.1 200 OK\r\n\
            Server: test-accept\r\n\
            Content-Encoding: gzip\r\n\
            Content-Length: {}\r\n\
            \r\n", &gzipped_content.len())
        .into_bytes();
    response.extend(&gzipped_content);

    let server = server! {
        request: b"\
            GET /gzip HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        chunk_size: chunk_size,
        write_timeout: Duration::from_millis(10),
        response: response
    };

    let mut core = Core::new().unwrap();

    let client = Client::new(&core.handle());

    let res_future = client.get(&format!("http://{}/gzip", server.addr()))
        .send()
        .and_then(|mut res| {
            let body = mem::replace(res.body_mut(), Decoder::empty());
            body.concat2()
        })
        .and_then(|buf| {
            let body = ::std::str::from_utf8(&buf).unwrap();

            assert_eq!(body, &content);

            Ok(())
        });

    core.run(res_future).unwrap();
}
