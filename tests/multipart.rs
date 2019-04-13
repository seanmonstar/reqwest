extern crate env_logger;
extern crate reqwest;

#[macro_use]
mod support;

#[test]
fn text_part() {
    let _ = env_logger::try_init();

    let form = reqwest::multipart::Form::new()
        .text("foo", "bar");

    let expected_body = format!("\
        --{0}\r\n\
        Content-Disposition: form-data; name=\"foo\"\r\n\r\n\
        bar\r\n\
        --{0}--\r\n\
    ", form.boundary());

    let server = server! {
        request: format!("\
            POST /multipart/1 HTTP/1.1\r\n\
            content-type: multipart/form-data; boundary={}\r\n\
            content-length: 125\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
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
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

#[test]
fn file() {
    let _ = env_logger::try_init();

    let form = reqwest::multipart::Form::new()
        .file("foo", "Cargo.lock").unwrap();

    let fcontents = ::std::fs::read_to_string("Cargo.lock").unwrap();

    let expected_body = format!("\
        --{0}\r\n\
        Content-Disposition: form-data; name=\"foo\"; filename=\"Cargo.lock\"\r\n\
        Content-Type: application/octet-stream\r\n\r\n\
        {1}\r\n\
        --{0}--\r\n\
    ", form.boundary(), fcontents);

    let server = server! {
        request: format!("\
            POST /multipart/2 HTTP/1.1\r\n\
            content-type: multipart/form-data; boundary={}\r\n\
            content-length: {}\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            {}\
            ", form.boundary(), expected_body.len(), expected_body),
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: multipart\r\n\
            Content-Length: 0\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/multipart/2", server.addr());

    let res = reqwest::Client::new()
        .post(&url)
        .multipart(form)
        .send()
        .unwrap();

    assert_eq!(res.url().as_str(), &url);
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}
