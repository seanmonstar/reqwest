extern crate env_logger;
extern crate reqwest;

#[macro_use]
mod support;

use std::io::Read;
use std::time::Duration;

#[test]
fn timeout_closes_connection() {
    let _ = env_logger::try_init();

    // Make Client drop *after* the Server, so the background doesn't
    // close too early.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();

    let server = server! {
        request: b"\
            GET /closes HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            ",
        read_timeout: Duration::from_secs(2),
        read_closes: true
    };

    let url = format!("http://{}/closes", server.addr());
    let err = client
        .get(&url)
        .send()
        .unwrap_err();

    assert_eq!(err.get_ref().unwrap().to_string(), "timed out");
    assert_eq!(err.url().map(|u| u.as_str()), Some(url.as_str()));
}

#[test]
fn write_timeout_large_body() {
    let _ = env_logger::try_init();
    let body = String::from_utf8(vec![b'x'; 20_000]).unwrap();
    let len = 8192;

    // Make Client drop *after* the Server, so the background doesn't
    // close too early.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();

    let server = server! {
        request: format!("\
            POST /write-timeout HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            content-length: {}\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            {}\
            ", body.len(), body),
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            ",
        read_timeout: Duration::from_secs(2),
        read_closes: true
    };

    let cursor = ::std::io::Cursor::new(body.into_bytes());
    let url = format!("http://{}/write-timeout", server.addr());
    let err = client
        .post(&url)
        .body(reqwest::Body::sized(cursor, len as u64))
        .send()
        .unwrap_err();

    assert_eq!(err.get_ref().unwrap().to_string(), "timed out");
    assert_eq!(err.url().map(|u| u.as_str()), Some(url.as_str()));
}


#[test]
fn test_response_timeout() {
    let _ = env_logger::try_init();
    let server = server! {
        request: b"\
            GET /response-timeout HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 0\r\n\
            ",
        response_timeout: Duration::from_secs(1)
    };

    let url = format!("http://{}/response-timeout", server.addr());
    let err = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap()
        .get(&url)
        .send()
        .unwrap_err();

    assert_eq!(err.get_ref().unwrap().to_string(), "timed out");
    assert_eq!(err.url().map(|u| u.as_str()), Some(url.as_str()));
}

#[test]
fn test_read_timeout() {
    let _ = env_logger::try_init();
    let server = server! {
        request: b"\
            GET /read-timeout HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            ",
        write_timeout: Duration::from_secs(1)
    };

    let url = format!("http://{}/read-timeout", server.addr());
    let mut res = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap()
        .get(&url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"5");

    let mut buf = [0; 1024];
    let err = res.read(&mut buf).unwrap_err();
    assert_eq!(err.to_string(), "timed out");
}
