extern crate reqwest;

#[macro_use]
mod support;

#[test]
fn test_http_proxy() {
    let server = server! {
        request: b"\
            GET http://hyper.rs/prox HTTP/1.1\r\n\
            Host: hyper.rs\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Accept-Encoding: gzip\r\n\
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
        .unwrap()
        .proxy(reqwest::Proxy::http(&proxy).unwrap())
        .build()
        .unwrap()
        .get(url)
        .unwrap()
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), url);
    assert_eq!(res.status(), reqwest::StatusCode::Ok);
    assert_eq!(res.headers().get(),
               Some(&reqwest::header::Server::new("proxied")));
}
