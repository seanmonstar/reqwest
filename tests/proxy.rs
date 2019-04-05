extern crate reqwest;

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
