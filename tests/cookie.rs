extern crate reqwest;

#[macro_use]
mod support;

#[test]
fn cookie_response_accessor() {
    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
    let client = reqwest::async::Client::new();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Set-Cookie: key=val\r\n\
            Set-Cookie: expires=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT\r\n\
            Set-Cookie: path=1; Path=/the-path\r\n\
            Set-Cookie: maxage=1; Max-Age=100\r\n\
            Set-Cookie: domain=1; Domain=mydomain\r\n\
            Set-Cookie: secure=1; Secure\r\n\
            Set-Cookie: httponly=1; HttpOnly\r\n\
            Set-Cookie: samesitelax=1; SameSite=Lax\r\n\
            Set-Cookie: samesitestrict=1; SameSite=Strict\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/", server.addr());
    let res = rt.block_on(client.get(&url).send()).unwrap();

    let cookies = res.cookies().collect::<Vec<_>>();

    // key=val
    assert_eq!(cookies[0].name(), "key");
    assert_eq!(cookies[0].value(), "val");

    // expires
    assert_eq!(cookies[1].name(), "expires");
    assert_eq!(
        cookies[1].expires().unwrap(), 
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1445412480)
    );

    // path
    assert_eq!(cookies[2].name(), "path");
    assert_eq!(cookies[2].path().unwrap(), "/the-path");

    // max-age
    assert_eq!(cookies[3].name(), "maxage");
    assert_eq!(cookies[3].max_age().unwrap(), std::time::Duration::from_secs(100));

    // domain
    assert_eq!(cookies[4].name(), "domain");
    assert_eq!(cookies[4].domain().unwrap(), "mydomain");

    // secure
    assert_eq!(cookies[5].name(), "secure");
    assert_eq!(cookies[5].secure(), true);

    // httponly
    assert_eq!(cookies[6].name(), "httponly");
    assert_eq!(cookies[6].http_only(), true);

    // samesitelax
    assert_eq!(cookies[7].name(), "samesitelax");
    assert!(cookies[7].same_site_lax());

    // samesitestrict
    assert_eq!(cookies[8].name(), "samesitestrict");
    assert!(cookies[8].same_site_strict());
}

#[test]
fn cookie_store_simple() {
    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
    let client = reqwest::async::Client::builder().cookie_store(true).build().unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Set-Cookie: key=val; HttpOnly\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            cookie: key=val\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();
}

#[test]
fn cookie_store_overwrite_existing() {
    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
    let client = reqwest::async::Client::builder().cookie_store(true).build().unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Set-Cookie: key=val\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            cookie: key=val\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Set-Cookie: key=val2\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            cookie: key=val2\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();
}

#[test]
fn cookie_store_max_age() {
    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
    let client = reqwest::async::Client::builder().cookie_store(true).build().unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Set-Cookie: key=val; Max-Age=0\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();
}

#[test]
fn cookie_store_expires() {
    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
    let client = reqwest::async::Client::builder().cookie_store(true).build().unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Set-Cookie: key=val; Expires=Wed, 21 Oct 2015 07:28:00 GMT\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();
}

#[test]
fn cookie_store_path() {
    let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
    let client = reqwest::async::Client::builder().cookie_store(true).build().unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Set-Cookie: key=val; Path=/subpath\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();

    let server = server! {
        request: b"\
            GET / HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();

    let server = server! {
        request: b"\
            GET /subpath HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            cookie: key=val\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    let url = format!("http://{}/subpath", server.addr());
    rt.block_on(client.get(&url).send()).unwrap();
}
