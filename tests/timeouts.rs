extern crate reqwest;

#[macro_use]
mod support;

use std::io::Read;
use std::time::Duration;

#[test]
fn test_write_timeout() {
    let server = server! {
        request: b"\
            POST /write-timeout HTTP/1.1\r\n\
            Host: $HOST\r\n\
            Content-Length: 5\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            Hello\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            ",
        read_timeout: Duration::from_secs(1)
    };

    let url = format!("http://{}/write-timeout", server.addr());
    let err = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap()
        .post(&url)
        .header(reqwest::header::CONTENT_LENGTH, reqwest::header::HeaderValue::from_static("5"))
        .body(reqwest::Body::new(&b"Hello"[..]))
        .send()
        .unwrap_err();

    assert_eq!(err.get_ref().unwrap().to_string(), "timed out");
    assert_eq!(err.url().map(|u| u.as_str()), Some(url.as_str()));
}


#[test]
fn test_response_timeout() {
    let server = server! {
        request: b"\
            GET /response-timeout HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
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
    let server = server! {
        request: b"\
            GET /read-timeout HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
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
