extern crate reqwest;

#[macro_use] mod server;

use std::io::Read;

#[test]
fn test_get() {
    let server = server! {
        request: b"\
            GET /1 HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let mut res = reqwest::get(&format!("http://{}/1", server.addr())).unwrap();
    assert_eq!(res.status(), &reqwest::StatusCode::Ok);
    assert_eq!(res.version(), &reqwest::HttpVersion::Http11);
    assert_eq!(res.headers().get(), Some(&reqwest::header::Server("test".to_string())));
    assert_eq!(res.headers().get(), Some(&reqwest::header::ContentLength(0)));

    let mut buf = [0; 1024];
    let n = res.read(&mut buf).unwrap();
    assert_eq!(n, 0)
}

#[test]
fn test_redirect_302_changes_post_to_get() {

    let redirect = server! {
        request: b"\
            POST /302 HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Content-Length: 0\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 302 Found\r\n\
            Server: test-redirect\r\n\
            Content-Length: 0\r\n\
            Location: /dst\r\n\
            Connection: close\r\n\
            \r\n\
            ",

        request: b"\
            GET /dst HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Referer: http://$HOST/302\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: test-dst\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let client = reqwest::Client::new().unwrap();
    let res = client.post(&format!("http://{}/302", redirect.addr()))
        .send()
        .unwrap();
    assert_eq!(res.status(), &reqwest::StatusCode::Ok);
    assert_eq!(res.headers().get(), Some(&reqwest::header::Server("test-dst".to_string())));

}
