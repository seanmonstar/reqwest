#![cfg(feature="unstable")]

extern crate futures;
extern crate libflate;
extern crate reqwest;
extern crate tokio;

#[macro_use]
mod support;

use reqwest::unstable::async::Client;
use futures::{Future, Stream};
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
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        chunk_size: chunk_size,
        write_timeout: Duration::from_millis(10),
        response: response
    };

    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");

    let client = Client::new();

    let res_future = client.get(&format!("http://{}/gzip", server.addr()))
        .send()
        .and_then(|res| {
            let body = res.into_body();
            body.concat2()
        })
        .and_then(|buf| {
            let body = ::std::str::from_utf8(&buf).unwrap();

            assert_eq!(body, &content);

            Ok(())
        });

    rt.block_on(res_future).unwrap();
}
