extern crate reqwest;
extern crate libflate;

#[macro_use] mod server;

use std::io::Read;
use std::io::prelude::*;

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
    assert_eq!(res.status(), &reqwest::StatusCode::Ok);
    assert_eq!(res.version(), &reqwest::HttpVersion::Http11);
    assert_eq!(res.headers().get(), Some(&reqwest::header::Server("test".to_string())));
    assert_eq!(res.headers().get(), Some(&reqwest::header::ContentLength(0)));

    let mut buf = [0; 1024];
    let n = res.read(&mut buf).unwrap();
    assert_eq!(n, 0)
}

#[test]
fn test_redirect_301_and_302_and_303_changes_post_to_get() {
    let client = reqwest::Client::new().unwrap();
    let codes = [301, 302, 303];

    for code in codes.iter() {
        let redirect = server! {
            request: format!("\
                POST /{} HTTP/1.1\r\n\
                Host: $HOST\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
                Content-Length: 0\r\n\
                \r\n\
                ", code),
            response: format!("\
                HTTP/1.1 {} reason\r\n\
                Server: test-redirect\r\n\
                Content-Length: 0\r\n\
                Location: /dst\r\n\
                Connection: close\r\n\
                \r\n\
                ", code),

            request: format!("\
                GET /dst HTTP/1.1\r\n\
                Host: $HOST\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
                Referer: http://$HOST/{}\r\n\
                \r\n\
                ", code),
            response: b"\
                HTTP/1.1 200 OK\r\n\
                Server: test-dst\r\n\
                Content-Length: 0\r\n\
                \r\n\
                "
        };

        let url = format!("http://{}/{}", redirect.addr(), code);
        let dst = format!("http://{}/{}", redirect.addr(), "dst");
        let res = client.post(&url)
            .send()
            .unwrap();
        assert_eq!(res.url().as_str(), dst);
        assert_eq!(res.status(), &reqwest::StatusCode::Ok);
        assert_eq!(res.headers().get(), Some(&reqwest::header::Server("test-dst".to_string())));
    }
}

#[test]
fn test_redirect_307_and_308_tries_to_post_again() {
    let client = reqwest::Client::new().unwrap();
    let codes = [307, 308];
    for code in codes.iter() {
        let redirect = server! {
            request: format!("\
                POST /{} HTTP/1.1\r\n\
                Host: $HOST\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
                Content-Length: 5\r\n\
                \r\n\
                Hello\
                ", code),
            response: format!("\
                HTTP/1.1 {} reason\r\n\
                Server: test-redirect\r\n\
                Content-Length: 0\r\n\
                Location: /dst\r\n\
                Connection: close\r\n\
                \r\n\
                ", code),

            request: format!("\
                POST /dst HTTP/1.1\r\n\
                Host: $HOST\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
                Referer: http://$HOST/{}\r\n\
                Content-Length: 5\r\n\
                \r\n\
                Hello\
                ", code),
            response: b"\
                HTTP/1.1 200 OK\r\n\
                Server: test-dst\r\n\
                Content-Length: 0\r\n\
                \r\n\
                "
        };

        let url = format!("http://{}/{}", redirect.addr(), code);
        let dst = format!("http://{}/{}", redirect.addr(), "dst");
        let res = client.post(&url)
            .body("Hello")
            .send()
            .unwrap();
        assert_eq!(res.url().as_str(), dst);
        assert_eq!(res.status(), &reqwest::StatusCode::Ok);
        assert_eq!(res.headers().get(), Some(&reqwest::header::Server("test-dst".to_string())));
    }
}

#[test]
fn test_redirect_307_does_not_try_if_reader_cannot_reset() {
    let client = reqwest::Client::new().unwrap();
    let codes = [307, 308];
    for &code in codes.iter() {
        let redirect = server! {
            request: format!("\
                POST /{} HTTP/1.1\r\n\
                Host: $HOST\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
                Transfer-Encoding: chunked\r\n\
                \r\n\
                5\r\n\
                Hello\r\n\
                0\r\n\r\n\
                ", code),
            response: format!("\
                HTTP/1.1 {} reason\r\n\
                Server: test-redirect\r\n\
                Content-Length: 0\r\n\
                Location: /dst\r\n\
                Connection: close\r\n\
                \r\n\
                ", code)
        };

        let url = format!("http://{}/{}", redirect.addr(), code);
        let res = client.post(&url)
            .body(reqwest::Body::new(&b"Hello"[..]))
            .send()
            .unwrap();
        assert_eq!(res.url().as_str(), url);
        assert_eq!(res.status(), &reqwest::StatusCode::from_u16(code));
    }
}

#[test]
fn test_redirect_policy_can_return_errors() {
    let server = server! {
        request: b"\
            GET /loop HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 302 Found\r\n\
            Server: test\r\n\
            Location: /loop
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let err = reqwest::get(&format!("http://{}/loop", server.addr())).unwrap_err();
    assert!(err.is_redirect());
}

#[test]
fn test_redirect_policy_can_stop_redirects_without_an_error() {
    let server = server! {
        request: b"\
            GET /no-redirect HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 302 Found\r\n\
            Server: test-dont\r\n\
            Location: /dont
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let mut client = reqwest::Client::new().unwrap();
    client.redirect(reqwest::RedirectPolicy::none());

    let url = format!("http://{}/no-redirect", server.addr());
    let res = client.get(&url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), &reqwest::StatusCode::Found);
    assert_eq!(res.headers().get(), Some(&reqwest::header::Server("test-dont".to_string())));
}

#[test]
fn test_referer_is_not_set_if_disabled() {
    let server = server! {
        request: b"\
            GET /no-refer HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 302 Found\r\n\
            Server: test-no-referer\r\n\
            Content-Length: 0\r\n\
            Location: /dst\r\n\
            Connection: close\r\n\
            \r\n\
            ",

        request: b"\
            GET /dst HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test-dst\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let mut client = reqwest::Client::new().unwrap();
    client.referer(false);
    client.get(&format!("http://{}/no-refer", server.addr()))
        .send().unwrap();
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

    let res = client.get(&format!("http://{}/accept", server.addr()))
        .header(reqwest::header::Accept::json())
        .send()
        .unwrap();

    assert_eq!(res.status(), &reqwest::StatusCode::Ok);
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
        .header(reqwest::header::AcceptEncoding(
            vec![reqwest::header::qitem(reqwest::header::Encoding::Identity)]
        ))
        .send()
        .unwrap();

    assert_eq!(res.status(), &reqwest::StatusCode::Ok);
}

#[test]
fn test_gzip_response() {
    let mut encoder = ::libflate::gzip::Encoder::new(Vec::new()).unwrap();
    match encoder.write(b"test request") {
        Ok(n) => assert!(n > 0, "Failed to write to encoder."),
        _ => panic!("Failed to gzip encode string.")
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
    let mut res = reqwest::get(&format!("http://{}/gzip", server.addr()))
        .unwrap();

    let mut body = ::std::string::String::new();
    match res.read_to_string(&mut body) {
        Ok(n) => assert!(n > 0, "Failed to write to buffer."),
        _ => panic!("Failed to write to buffer.")
    };

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
    let mut res = client.head(&format!("http://{}/gzip", server.addr()))
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

    let mut res = reqwest::get(&format!("http://{}/gzip", server.addr()))
        .unwrap();
    // this tests that the request.send() didn't error, but that the error
    // is in reading the body

    let mut body = ::std::string::String::new();
    res.read_to_string(&mut body).unwrap_err();
}
