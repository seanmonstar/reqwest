extern crate reqwest;

#[macro_use]
mod support;

#[test]
fn test_multipart() {
    let form = reqwest::multipart::Form::new()
        .text("foo", "bar");

    let expected_body = format!("\
        --{0}\r\n\
        Content-Disposition: form-data; name=\"foo\"\r\n\r\n\
        bar\r\n\
        --{0}--\
    ", form.boundary());

    let server = server! {
        request: format!("\
            POST /multipart/1 HTTP/1.1\r\n\
            Host: $HOST\r\n\
            User-Agent: $USERAGENT\r\n\
            Accept: */*\r\n\
            Content-Type: multipart/form-data; boundary={}\r\n\
            Content-Length: 123\r\n\
            Accept-Encoding: gzip\r\n\
            \r\n\
            {}\
            ", form.boundary(), expected_body),
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: multipart\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/multipart/1", server.addr());

    let res = reqwest::Client::new()
        .post(&url)
        .multipart(form)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::Ok);
}
