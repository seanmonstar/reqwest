extern crate reqwest;

#[macro_use]
mod support;

use std::io::Read;

#[test]
fn test_get() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/1", server.addr());
    let mut res = reqwest::get(&url).unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::Ok);
    assert_eq!(res.headers().get(),
               Some(&reqwest::header::Server::new("test".to_string())));
    assert_eq!(res.headers().get(),
               Some(&reqwest::header::ContentLength(0)));

    let mut buf = [0; 1024];
    let n = res.read(&mut buf).unwrap();
    assert_eq!(n, 0)
}

#[test]
fn test_post() {
    let server = server! {
        request: b"\
            POST /2 HTTP/1.1\r\n\
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
            Server: post\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/2", server.addr());
    let mut res = reqwest::Client::new()
        .unwrap()
        .post(&url)
        .unwrap()
        .body("Hello")
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::Ok);
    assert_eq!(res.headers().get(),
               Some(&reqwest::header::Server::new("post")));
    assert_eq!(res.headers().get(),
               Some(&reqwest::header::ContentLength(0)));

    let mut buf = [0; 1024];
    let n = res.read(&mut buf).unwrap();
    assert_eq!(n, 0)
}

/// Calling `Response::error_for_status`` on a response with status in 4xx
/// returns a error.
#[test]
fn test_error_for_status_4xx() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 400 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/1", server.addr());
    let res = reqwest::get(&url).unwrap();

    let err = res.error_for_status().err().unwrap();
    assert!(err.is_client_error());
    assert_eq!(err.status(), Some(reqwest::StatusCode::BadRequest));
}

/// Calling `Response::error_for_status`` on a response with status in 5xx
/// returns a error.
#[test]
fn test_error_for_status_5xx() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 500 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/1", server.addr());
    let res = reqwest::get(&url).unwrap();

    let err = res.error_for_status().err().unwrap();
    assert!(err.is_server_error());
    assert_eq!(err.status(), Some(reqwest::StatusCode::InternalServerError));
}
