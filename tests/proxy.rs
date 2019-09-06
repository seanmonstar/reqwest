extern crate reqwest;

#[macro_use]
mod support;

use std::env;

#[test]
fn http_proxy() {
    let server = server! {
        request: b"\
            GET http://hyper.rs/prox HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: hyper.rs\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: proxied\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let proxy = format!("http://{}", server.addr());

    let url = "http://hyper.rs/prox";
    let res = reqwest::Client::builder()
        .proxy(reqwest::Proxy::http(&proxy).unwrap())
        .build()
        .unwrap()
        .get(url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"proxied");
}

#[test]
fn http_proxy_basic_auth() {
    let server = server! {
        request: b"\
            GET http://hyper.rs/prox HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            proxy-authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==\r\n\
            host: hyper.rs\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: proxied\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let proxy = format!("http://{}", server.addr());

    let url = "http://hyper.rs/prox";
    let res = reqwest::Client::builder()
        .proxy(
            reqwest::Proxy::http(&proxy)
            .unwrap()
            .basic_auth("Aladdin", "open sesame")
        )
        .build()
        .unwrap()
        .get(url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"proxied");
}

#[test]
fn http_proxy_basic_auth_parsed() {
    let server = server! {
        request: b"\
            GET http://hyper.rs/prox HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            proxy-authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==\r\n\
            host: hyper.rs\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: proxied\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let proxy = format!("http://Aladdin:open sesame@{}", server.addr());

    let url = "http://hyper.rs/prox";
    let res = reqwest::Client::builder()
        .proxy(
            reqwest::Proxy::http(&proxy).unwrap()
        )
        .build()
        .unwrap()
        .get(url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"proxied");
}

#[test]
fn test_no_proxy() {
    let server = server! {
        request: b"\
            GET /4 HTTP/1.1\r\n\
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
    let proxy = format!("http://{}", server.addr());
    let url = format!("http://{}/4", server.addr());

    // set up proxy and use no_proxy to clear up client builder proxies.
    let res = reqwest::Client::builder()
        .proxy(
            reqwest::Proxy::http(&proxy).unwrap()
        )
        .no_proxy()
        .build()
        .unwrap()
        .get(&url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

#[test]
fn test_using_system_proxy() {
    let server = server! {
        request: b"\
            GET http://hyper.rs/prox HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: hyper.rs\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: proxied\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };
    // save system setting first.
    let system_proxy = env::var("http_proxy");
    // set-up http proxy.
    env::set_var("http_proxy", format!("http://{}", server.addr()));

    let url = "http://hyper.rs/prox";
    let res = reqwest::Client::builder()
        .use_sys_proxy()
        .build()
        .unwrap()
        .get(url)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"proxied");

    // reset user setting.
    match system_proxy {
        Err(_) => env::remove_var("http_proxy"),
        Ok(proxy) => env::set_var("http_proxy", proxy)
    }
}
