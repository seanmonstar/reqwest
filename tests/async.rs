extern crate futures;
extern crate libflate;
extern crate reqwest;
extern crate hyper;
extern crate tokio;
extern crate bytes;

#[macro_use]
mod support;

use std::io::{self, Write};
use std::time::Duration;

use futures::{Future, Stream};
use tokio::runtime::current_thread::Runtime;

use reqwest::async::Client;
use reqwest::async::multipart::{Form, Part};

use bytes::Bytes;

#[test]
fn gzip_response() {
    gzip_case(10_000, 4096);
}

#[test]
fn gzip_single_byte_chunks() {
    gzip_case(10, 1);
}

#[test]
fn response_text() {
    let _ = env_logger::try_init();

    let server = server! {
        request: b"\
            GET /text HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            "
    };

    let mut rt = Runtime::new().expect("new rt");

    let client = Client::new();

    let res_future = client.get(&format!("http://{}/text", server.addr()))
        .send()
        .and_then(|mut res| res.text())
        .and_then(|text| {
            assert_eq!("Hello", text);
            Ok(())
        });

    rt.block_on(res_future).unwrap();
}

#[test]
fn response_json() {
    let _ = env_logger::try_init();

    let server = server! {
        request: b"\
            GET /json HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 7\r\n\
            \r\n\
            \"Hello\"\
            "
    };

    let mut rt = Runtime::new().expect("new rt");

    let client = Client::new();

    let res_future = client.get(&format!("http://{}/json", server.addr()))
        .send()
        .and_then(|mut res| res.json::<String>())
        .and_then(|text| {
            assert_eq!("Hello", text);
            Ok(())
        });

    rt.block_on(res_future).unwrap();
}

#[test]
fn multipart() {
    let _ = env_logger::try_init();

    let stream = futures::stream::once::<_, hyper::Error>(Ok(hyper::Chunk::from("part1 part2".to_owned())));
    let part = Part::stream(stream);

    let form = Form::new()
        .text("foo", "bar")
        .part("part_stream", part);

    let expected_body = format!("\
        24\r\n\
        --{0}\r\n\r\n\
        2E\r\n\
        Content-Disposition: form-data; name=\"foo\"\r\n\r\n\r\n\
        3\r\n\
        bar\r\n\
        2\r\n\
        \r\n\r\n\
        24\r\n\
        --{0}\r\n\r\n\
        36\r\n\
        Content-Disposition: form-data; name=\"part_stream\"\r\n\r\n\r\n\
        B\r\n\
        part1 part2\r\n\
        2\r\n\
        \r\n\r\n\
        26\r\n\
        --{0}--\r\n\r\n\
        0\r\n\r\n\
    ", form.boundary());

    let server = server! {
        request: format!("\
            POST /multipart/1 HTTP/1.1\r\n\
            content-type: multipart/form-data; boundary={}\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            transfer-encoding: chunked\r\n\
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

    let mut rt = Runtime::new().expect("new rt");

    let client = Client::new();

    let res_future = client.post(&url)
        .multipart(form)
        .send()
        .and_then(|res| {
            assert_eq!(res.url().as_str(), &url);
            assert_eq!(res.status(), reqwest::StatusCode::OK);

            Ok(())
        });

    rt.block_on(res_future).unwrap();
}

#[test]
fn request_timeout() {
    let _ = env_logger::try_init();

    let server = server! {
        request: b"\
            GET /slow HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            ",
        read_timeout: Duration::from_secs(2)
    };

    let mut rt = Runtime::new().expect("new rt");

    let client = Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();

    let url = format!("http://{}/slow", server.addr());
    let fut = client
        .get(&url)
        .send();

    let err = rt.block_on(fut).unwrap_err();

    assert!(err.is_timeout());
    assert_eq!(err.url().map(|u| u.as_str()), Some(url.as_str()));
}

#[test]
fn response_timeout() {
    let _ = env_logger::try_init();

    let server = server! {
        request: b"\
            GET /slow HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 5\r\n\
            \r\n\
            Hello\
            ",
        write_timeout: Duration::from_secs(2)
    };

    let mut rt = Runtime::new().expect("new rt");

    let client = Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();

    let url = format!("http://{}/slow", server.addr());
    let fut = client
        .get(&url)
        .send()
        .and_then(|res| res.into_body().concat2());

    let err = rt.block_on(fut).unwrap_err();

    assert!(err.is_timeout());
}

fn gzip_case(response_size: usize, chunk_size: usize) {
    let content: String = (0..response_size).into_iter().map(|i| format!("test {}", i)).collect();
    let mut encoder = ::libflate::gzip::Encoder::new(Vec::new()).unwrap();
    match encoder.write(content.as_bytes()) {
        Ok(n) => assert!(n > 0, "Failed to write to encoder."),
        _ => panic!("Failed to gzip encode string."),
    };

    let gzipped_content = encoder.finish().into_result().unwrap();

    let mut response = format!("\
            HTTP/1.1 200 OK\r\n\
            Server: test-accept\r\n\
            Content-Encoding: gzip\r\n\
            Content-Length: {}\r\n\
            \r\n", &gzipped_content.len())
        .into_bytes();
    response.extend(&gzipped_content);

    let server = server! {
        request: b"\
            GET /gzip HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        chunk_size: chunk_size,
        write_timeout: Duration::from_millis(10),
        response: response
    };

    let mut rt = Runtime::new().expect("new rt");

    let client = Client::new();

    let res_future = client.get(&format!("http://{}/gzip", server.addr()))
        .send()
        .and_then(|res| {
            let body = res.into_body();
            body.concat2()
        })
        .and_then(|buf| {
            let body = ::std::str::from_utf8(&buf).unwrap();

            assert_eq!(body, &content);

            Ok(())
        });

    rt.block_on(res_future).unwrap();
}

#[test]
fn body_stream() {
    let _ = env_logger::try_init();

    let source: Box<dyn Stream<Item = Bytes, Error = io::Error> + Send>
        = Box::new(futures::stream::iter_ok::<_, io::Error>(
            vec![Bytes::from_static(b"123"), Bytes::from_static(b"4567")]));

    let expected_body = "3\r\n123\r\n4\r\n4567\r\n0\r\n\r\n";

    let server = server! {
        request: format!("\
            POST /post HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            transfer-encoding: chunked\r\n\
            \r\n\
            {}\
            ", expected_body),
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Server: post\r\n\
            Content-Length: 7\r\n\
            \r\n\
            "
    };

    let url = format!("http://{}/post", server.addr());

    let mut rt = Runtime::new().expect("new rt");

    let client = Client::new();

    let res_future = client.post(&url)
        .body(source)
        .send()
        .and_then(|res| {
            assert_eq!(res.url().as_str(), &url);
            assert_eq!(res.status(), reqwest::StatusCode::OK);

            Ok(())
        });

    rt.block_on(res_future).unwrap();
}

#[test]
fn blocking_inside_async_context() {
    let _ = env_logger::try_init();

    let server = server! {
        request: b"\
            GET /enter HTTP/1.1\r\n\
            user-agent: $USERAGENT\r\n\
            accept: */*\r\n\
            accept-encoding: gzip\r\n\
            host: $HOST\r\n\
            \r\n\
            ",
        response: b"\
            HTTP/1.1 200 OK\r\n\
            Content-Length: 18\r\n\
            \r\n\
            Blocky McBlockface\
            "
    };

    let mut rt = Runtime::new().expect("new rt");
    let url = format!("http://{}/enter", server.addr());

    rt.block_on(futures::future::lazy(|| {
        let mut resp = reqwest::get(&url)?;
        let text = resp.text()?;
        assert_eq!("Blocky McBlockface", text);
        Ok::<_, reqwest::Error>(())
    })).unwrap();
}
