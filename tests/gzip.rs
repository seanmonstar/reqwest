extern crate reqwest;
extern crate libflate;

#[macro_use]
mod support;

use std::io::{Read, Write};

#[test]
fn test_gzip_response() {
    let mut encoder = ::libflate::gzip::Encoder::new(Vec::new()).unwrap();
    match encoder.write(b"test request") {
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
        response: response
    };
    let mut res = reqwest::get(&format!("http://{}/gzip", server.addr())).unwrap();

    let mut body = String::new();
    res.read_to_string(&mut body).unwrap();

    assert_eq!(body, "test request");
}

#[test]
fn test_gzip_empty_body() {
    let server = server! {
        request: b"\
            HEAD /gzip HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test-accept\r\n\
            Content-Encoding: gzip\r\n\
            Content-Length: 100\r\n\
            \r\n"
    };

    let client = reqwest::Client::new().unwrap();
    let mut res = client
        .head(&format!("http://{}/gzip", server.addr()))
        .unwrap()
        .send()
        .unwrap();

    let mut body = ::std::string::String::new();
    res.read_to_string(&mut body).unwrap();

    assert_eq!(body, "");
}

#[test]
fn test_gzip_invalid_body() {
    let server = server! {
        request: b"\
            GET /gzip HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test-accept\r\n\
            Content-Encoding: gzip\r\n\
            Content-Length: 100\r\n\
            \r\n\
            0"
    };

    let mut res = reqwest::get(&format!("http://{}/gzip", server.addr())).unwrap();
    // this tests that the request.send() didn't error, but that the error
    // is in reading the body

    let mut body = ::std::string::String::new();
    res.read_to_string(&mut body).unwrap_err();
}

#[test]
fn test_accept_header_is_not_changed_if_set() {
    let server = server! {
        request: b"\
            GET /accept HTTP/1.1\r\n\
            Host: $HOST\r\n\
            Accept: application/json\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test-accept\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let client = reqwest::Client::new().unwrap();

    let res = client
        .get(&format!("http://{}/accept", server.addr()))
        .unwrap()
        .header(reqwest::header::Accept::json())
        .send()
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::Ok);
}

#[test]
fn test_accept_encoding_header_is_not_changed_if_set() {
    let server = server! {
        request: b"\
            GET /accept-encoding HTTP/1.1\r\n\
            Host: $HOST\r\n\
            Accept-Encoding: identity\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test-accept-encoding\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let client = reqwest::Client::new().unwrap();

    let res = client.get(&format!("http://{}/accept-encoding", server.addr()))
        .unwrap()
        .header(reqwest::header::AcceptEncoding(
            vec![reqwest::header::qitem(reqwest::header::Encoding::Identity)]
        ))
        .send()
        .unwrap();

    assert_eq!(res.status(), reqwest::StatusCode::Ok);
}
