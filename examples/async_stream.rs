#![deny(warnings)]

#[macro_use]
extern crate futures;
extern crate bytes;
extern crate reqwest;
extern crate tokio;
extern crate tokio_threadpool;

use std::io::{self, Cursor};
use std::mem;
use std::path::Path;

use bytes::Bytes;
use futures::{Async, Future, Poll, Stream};
use reqwest::async::{Client, Decoder};
use tokio::fs::File;
use tokio::io::AsyncRead;

const CHUNK_SIZE: usize = 1024;

struct FileSource {
    inner: File,
}

impl FileSource {
    fn new(file: File) -> FileSource {
        FileSource { inner: file }
    }
}

impl Stream for FileSource {
    type Item = Bytes;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut buf = [0; CHUNK_SIZE];
        let size = try_ready!(self.inner.poll_read(&mut buf));
        if size > 0 {
            Ok(Async::Ready(Some(buf[0..size].into())))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

fn post<P>(path: P) -> impl Future<Item = (), Error = ()>
where
    P: AsRef<Path>,
{
    File::open(path.as_ref().to_owned())
        .map_err(|err| println!("request error: {}", err))
        .and_then(|file| {
            let source: Box<dyn Stream<Item = Bytes, Error = io::Error> + Send> =
                Box::new(FileSource::new(file));

            Client::new()
                .post("https://httpbin.org/post")
                .body(source)
                .send()
                .and_then(|mut res| {
                    println!("{}", res.status());

                    let body = mem::replace(res.body_mut(), Decoder::empty());
                    body.concat2()
                })
                .map_err(|err| println!("request error: {}", err))
                .map(|body| {
                    let mut body = Cursor::new(body);
                    let _ = io::copy(&mut body, &mut io::stdout()).map_err(|err| {
                        println!("stdout error: {}", err);
                    });
                })
        })
}

fn main() {
    let pool = tokio_threadpool::ThreadPool::new();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/LICENSE-APACHE");
    tokio::run(pool.spawn_handle(post(path)));
}
