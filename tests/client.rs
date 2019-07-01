extern crate reqwest;

#[macro_use]
mod support;

use std::io::Read;

#[test]
fn test_response_text() {
    let server = server! {
        request: b"\
            GET /text HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            "
    };

    let url = format!("http://{}/text", server.addr());
    let mut res = reqwest::get(&url).unwrap();
    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"5");

    let body = res.text().unwrap();
    assert_eq!(b"Hello", body.as_bytes());
}

#[test]
fn test_response_non_utf_8_text() {
    let server = server! {
        request: b"\
            GET /text HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 4\r\n\
            Content-Type: text/plain; charset=gbk\r\n\
            \r\n\
            \xc4\xe3\xba\xc3\
            "
    };

    let url = format!("http://{}/text", server.addr());
    let mut res = reqwest::get(&url).unwrap();
    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"4");

    let body = res.text().unwrap();
    assert_eq!("你好", &body);
    assert_eq!(b"\xe4\xbd\xa0\xe5\xa5\xbd", body.as_bytes());  // Now it's utf-8
}

#[test]
fn test_response_json() {
    let server = server! {
        request: b"\
            GET /json HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 7\r\n\
            \r\n\
            \"Hello\"\
            "
    };

    let url = format!("http://{}/json", server.addr());
    let mut res = reqwest::get(&url).unwrap();
    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"7");

    let body = res.json::<String>().unwrap();
    assert_eq!("Hello", body);
}

#[test]
fn test_response_copy_to() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            "
    };

    let url = format!("http://{}/1", server.addr());
    let mut res = reqwest::get(&url).unwrap();
    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"5");

    let mut buf: Vec<u8> = vec![];
    res.copy_to(&mut buf).unwrap();
    assert_eq!(b"Hello", buf.as_slice());
}

#[test]
fn test_get() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"0");
    assert_eq!(res.remote_addr(), Some(server.addr()));

    let mut buf = [0; 1024];
    let n = res.read(&mut buf).unwrap();
    assert_eq!(n, 0)
}

#[test]
fn test_post() {
    let server = server! {
        request: b"\
            POST /2 HTTP/1.1\r\n\
            content-length: 5\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
        .post(&url)
        .body("Hello")
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"post");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"0");

    let mut buf = [0; 1024];
    let n = res.read(&mut buf).unwrap();
    assert_eq!(n, 0)
}

#[test]
fn test_post_form() {
    let server = server! {
        request: b"\
            POST /form HTTP/1.1\r\n\
            content-type: application/x-www-form-urlencoded\r\n\
            content-length: 24\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            hello=world&sean=monstar\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: post-form\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let form = &[("hello", "world"), ("sean", "monstar")];

    let url = format!("http://{}/form", server.addr());
    let res = reqwest::Client::new()
        .post(&url)
        .form(form)
        .send()
        .expect("request send");

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

/// Calling `Response::error_for_status`` on a response with status in 4xx
/// returns a error.
#[test]
fn test_error_for_status_4xx() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
    assert_eq!(err.status(), Some(reqwest::StatusCode::BAD_REQUEST));
}

/// Calling `Response::error_for_status`` on a response with status in 5xx
/// returns a error.
#[test]
fn test_error_for_status_5xx() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
    assert_eq!(err.status(), Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
}

#[test]
fn test_default_headers() {
    use reqwest::header;
    let mut headers = header::HeaderMap::with_capacity(1);
    headers.insert(header::COOKIE, header::HeaderValue::from_static("a=b;c=d"));
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build().unwrap();

    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            cookie: a=b;c=d\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
    let res = client.get(&url).send().unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"0");

    let server = server! {
        request: b"\
            GET /2 HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            cookie: a=b;c=d\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/2", server.addr());
    let res = client.get(&url).send().unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"0");
}

#[test]
fn test_override_default_headers() {
    use reqwest::header;
    let mut headers = header::HeaderMap::with_capacity(1);
    headers.insert(header::AUTHORIZATION, header::HeaderValue::from_static("iamatoken"));
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build().unwrap();

    let server = server! {
        request: b"\
            GET /3 HTTP/1.1\r\n\
            authorization: secret\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/3", server.addr());
    let res = client.get(&url).header(header::AUTHORIZATION, header::HeaderValue::from_static("secret")).send().unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"0");

}

#[test]
fn test_appended_headers_not_overwritten() {
    let client = reqwest::Client::new();

    let server = server! {
        request: b"\
            GET /4 HTTP/1.1\r\n\
            accept: application/json\r\n\
            accept: application/json+hal\r\n\
            user-agent: $USERAGENT\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/4", server.addr());
    let res = client.get(&url).header(header::ACCEPT, "application/json").header(header::ACCEPT, "application/json+hal").send().unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"0");

    // make sure this also works with default headers
    use reqwest::header;
    let mut headers = header::HeaderMap::with_capacity(1);
    headers.insert(header::ACCEPT, header::HeaderValue::from_static("text/html"));
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build().unwrap();

    let server = server! {
        request: b"\
            GET /4 HTTP/1.1\r\n\
            accept: application/json\r\n\
            accept: application/json+hal\r\n\
            user-agent: $USERAGENT\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/4", server.addr());
    let res = client.get(&url).header(header::ACCEPT, "application/json").header(header::ACCEPT, "application/json+hal").send().unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test");
    assert_eq!(res.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &"0");
}
