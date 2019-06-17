extern crate reqwest;
#[macro_use]
extern crate lazy_static;

use std::env;
use std::sync::Mutex;

#[macro_use]
mod support;

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

lazy_static! {
    static ref LOCK: Mutex<()> = Mutex::new(());
}

#[test]
fn test_get_proxies() {
    let _l = LOCK.lock();
    // save system setting first.
    let system_proxy = env::var("http_pxoxy");

    // remove proxy.
    env::remove_var("http_proxy");
    // ensure that the proxy is removed
    assert_eq!(env::var("http_proxy").is_err(), true);
    assert_eq!(reqwest::get_proxies().len(), 0);

    // the system proxy setting url is invalid.
    env::set_var("http_proxy", "123465");
    // ensure that the proxy is set
    assert_eq!(env::var("http_proxy").is_ok(), true);
    assert_eq!(reqwest::get_proxies().len(), 0);

    // set valid proxy
    env::set_var("http_proxy", "http://127.0.0.1/");
    // ensure that the proxy is set
    assert_eq!(env::var("http_proxy").is_ok(), true);
    let proxies = reqwest::get_proxies();
    let http_target = proxies.get("http").unwrap().as_str();
    assert_eq!(http_target, "http://127.0.0.1/");

    // reset user setting.
    match system_proxy {
        Err(_) => env::remove_var("http_proxy"),
        Ok(proxy) => env::set_var("http_proxy", proxy)
    }
}

#[test]
fn test_system_proxy_is_used() {
    let _l = LOCK.lock();
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
    let system_proxy = env::var("http_pxoxy");

    // set-up http proxy.
    env::set_var("http_proxy", format!("http://{}", server.addr()));
    // ensure that the proxy is set
    assert_eq!(env::var("http_proxy").is_ok(), true);

    let url = "http://hyper.rs/prox";
    let res = reqwest::get(url).unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    assert_eq!(res.headers().get(reqwest::header::SERVER).unwrap(), &"proxied");

    // reset user setting.
    match system_proxy {
        Err(_) => env::remove_var("http_proxy"),
        Ok(proxy) => env::set_var("http_proxy", proxy)
    }
}
