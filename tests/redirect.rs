extern crate reqwest;

#[macro_use]
mod support;

#[test]
fn test_redirect_301_and_302_and_303_changes_post_to_get() {
    let client = reqwest::Client::new();
    let codes = [301, 302, 303];

    for code in codes.iter() {
        let redirect = server! {
            request: format!("\
                POST /{} HTTP/1.1\r\n\
                Host: $HOST\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
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
        assert_eq!(res.status(), reqwest::StatusCode::Ok);
        assert_eq!(res.headers().get(),
                   Some(&reqwest::header::Server::new("test-dst".to_string())));
    }
}

#[test]
fn test_redirect_307_and_308_tries_to_get_again() {
    let client = reqwest::Client::new();
    let codes = [307, 308];
    for code in codes.iter() {
        let redirect = server! {
            request: format!("\
                GET /{} HTTP/1.1\r\n\
                Host: $HOST\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
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
        let res = client.get(&url)
            .send()
            .unwrap();
        assert_eq!(res.url().as_str(), dst);
        assert_eq!(res.status(), reqwest::StatusCode::Ok);
        assert_eq!(res.headers().get(),
                   Some(&reqwest::header::Server::new("test-dst".to_string())));
    }
}

#[test]
fn test_redirect_307_and_308_tries_to_post_again() {
    let client = reqwest::Client::new();
    let codes = [307, 308];
    for code in codes.iter() {
        let redirect = server! {
            request: format!("\
                POST /{} HTTP/1.1\r\n\
                Host: $HOST\r\n\
                Content-Length: 5\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
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
                Content-Length: 5\r\n\
                User-Agent: $USERAGENT\r\n\
                Accept: */*\r\n\
                Accept-Encoding: gzip\r\n\
                Referer: http://$HOST/{}\r\n\
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
        assert_eq!(res.status(), reqwest::StatusCode::Ok);
        assert_eq!(res.headers().get(),
                   Some(&reqwest::header::Server::new("test-dst".to_string())));
    }
}

#[test]
fn test_redirect_307_does_not_try_if_reader_cannot_reset() {
    let client = reqwest::Client::new();
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
        let res = client
            .post(&url)
            .body(reqwest::Body::new(&b"Hello"[..]))
            .send()
            .unwrap();
        assert_eq!(res.url().as_str(), url);
        assert_eq!(res.status(), reqwest::StatusCode::try_from(code).unwrap());
    }
}



#[test]
fn test_redirect_removes_sensitive_headers() {
    let end_server = server! {
        request: b"\
            GET /otherhost HTTP/1.1\r\n\
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

    let mid_server = server! {
        request: b"\
            GET /sensitive HTTP/1.1\r\n\
            Host: $HOST\r\n\
            Cookie: foo=bar\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            ",
        response: format!("\
            HTTP/1.1 302 Found\r\n\
            Server: test\r\n\
            Location: http://{}/otherhost\r\n\
            Content-Length: 0\r\n\
            \r\n\
            ", end_server.addr())
    };

    let mut cookie = reqwest::header::Cookie::new();
    cookie.set("foo", "bar");
    reqwest::Client::builder()
        .referer(false)
        .build()
        .unwrap()
        .get(&format!("http://{}/sensitive", mid_server.addr()))
        .header(cookie)
        .send()
        .unwrap();
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
            Location: /loop\r\n\
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
            Location: /dont\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/no-redirect", server.addr());

    let res = reqwest::Client::builder()
        .redirect(reqwest::RedirectPolicy::none())
        .build()
        .unwrap()
        .get(&url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::Found);
    assert_eq!(res.headers().get(),
               Some(&reqwest::header::Server::new("test-dont".to_string())));
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
    reqwest::Client::builder()
        .referer(false)
        .build()
        .unwrap()
        //client
        .get(&format!("http://{}/no-refer", server.addr()))
        .send()
        .unwrap();
}
