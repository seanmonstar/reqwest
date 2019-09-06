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
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                host: $HOST\r\n\
                \r\n\
                ", code),
            response: format!("\
                HTTP/1.1 {} reason\r\n\
                Server: test-redirect\r\n\
                Content-Length: 0\r\n\
                Location: /dst\r\n\
                Connection: close\r\n\
                \r\n\
                ", code)
                ;

            request: format!("\
                GET /dst HTTP/1.1\r\n\
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                referer: http://$HOST/{}\r\n\
                host: $HOST\r\n\
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
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test-dst");
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
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                host: $HOST\r\n\
                \r\n\
                ", code),
            response: format!("\
                HTTP/1.1 {} reason\r\n\
                Server: test-redirect\r\n\
                Content-Length: 0\r\n\
                Location: /dst\r\n\
                Connection: close\r\n\
                \r\n\
                ", code)
                ;

            request: format!("\
                GET /dst HTTP/1.1\r\n\
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                referer: http://$HOST/{}\r\n\
                host: $HOST\r\n\
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
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test-dst");
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
                content-length: 5\r\n\
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                host: $HOST\r\n\
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
                ", code)
                ;

            request: format!("\
                POST /dst HTTP/1.1\r\n\
                content-length: 5\r\n\
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                referer: http://$HOST/{}\r\n\
                host: $HOST\r\n\
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
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test-dst");
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
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                host: $HOST\r\n\
                transfer-encoding: chunked\r\n\
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
        assert_eq!(res.status(), reqwest::StatusCode::from_u16(code).unwrap());
    }
}



#[test]
fn test_redirect_removes_sensitive_headers() {
    let end_server = server! {
        request: b"\
            GET /otherhost HTTP/1.1\r\n\
            accept-encoding: gzip\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
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

    let mid_server = server! {
        request: b"\
            GET /sensitive HTTP/1.1\r\n\
            cookie: foo=bar\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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

    reqwest::Client::builder()
        .referer(false)
        .build()
        .unwrap()
        .get(&format!("http://{}/sensitive", mid_server.addr()))
        .header(reqwest::header::COOKIE, reqwest::header::HeaderValue::from_static("foo=bar"))
        .send()
        .unwrap();
}

#[test]
fn test_redirect_policy_can_return_errors() {
    let server = server! {
        request: b"\
            GET /loop HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test-dont");
}

#[test]
fn test_referer_is_not_set_if_disabled() {
    let server = server! {
        request: b"\
            GET /no-refer HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 302 Found\r\n\
            Server: test-no-referer\r\n\
            Content-Length: 0\r\n\
            Location: /dst\r\n\
            Connection: close\r\n\
            \r\n\
            "
            ;

        request: b"\
            GET /dst HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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

#[test]
fn test_invalid_location_stops_redirect_gh484() {
    let server = server! {
        request: b"\
            GET /yikes HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 302 Found\r\n\
            Server: test-yikes\r\n\
            Location: http://www.yikes{KABOOM}\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/yikes", server.addr());

    let res = reqwest::get(&url).unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::FOUND);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test-yikes");
}

#[test]
fn test_redirect_302_with_set_cookies() {
    let code = 302;
    let client = reqwest::ClientBuilder::new().cookie_store(true).build().unwrap();
    let server = server! {
            request: format!("\
                GET /{} HTTP/1.1\r\n\
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                host: $HOST\r\n\
                \r\n\
                ", code),
            response: format!("\
                HTTP/1.1 {} reason\r\n\
                Server: test-redirect\r\n\
                Content-Length: 0\r\n\
                Location: /dst\r\n\
                Connection: close\r\n\
                Set-Cookie: key=value\r\n\
                \r\n\
                ", code)
                ;

            request: format!("\
                GET /dst HTTP/1.1\r\n\
                user-agent: $USERAGENT\r\n\
                accept: */*\r\n\
                accept-encoding: gzip\r\n\
                referer: http://$HOST/{}\r\n\
                cookie: key=value\r\n\
                host: $HOST\r\n\
                \r\n\
                ", code),
            response: b"\
                HTTP/1.1 200 OK\r\n\
                Server: test-dst\r\n\
                Content-Length: 0\r\n\
                \r\n\
                "
        };

    let url = format!("http://{}/{}", server.addr(), code);
    let dst = format!("http://{}/{}", server.addr(), "dst");
    let res = client.get(&url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), dst);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"test-dst");
}
