#![deny(warnings)]
use std::io::{self, Cursor};
use std::mem;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{Stream, TryStreamExt};
use reqwest::r#async::{Body, Client, Decoder};
use tokio_fs::File;
use tokio::io::AsyncRead;

use failure::Fail;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Io Error")]
    Io(#[fail(cause)] std::io::Error),
    #[fail(display = "Reqwest error")]
    Reqwest(#[fail(cause)] reqwest::Error),
}

unsafe impl Send for Error {}
unsafe impl Sync for Error {}

struct AsyncReadWrapper<T> {
    inner: T,
}

impl<T> AsyncReadWrapper<T> {
    fn inner(self: Pin<&mut Self>) -> Pin<&mut T> {
        unsafe {
            Pin::map_unchecked_mut(self, |x| &mut x.inner)
        }
    }
}

impl<T> Stream for AsyncReadWrapper<T>
    where T: AsyncRead
{
    type Item = Result<hyper::Chunk, failure::Compat<Error>>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut buf = vec![];
        loop {
            let mut read_buf = vec![];
            match self.as_mut().inner().as_mut().poll_read(cx, &mut read_buf) {
                Poll::Pending => {
                    if buf.is_empty() {
                        return Poll::Pending;
                    } else {
                        return Poll::Ready(Some(Ok(buf.into())));
                    }
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Some(Err(Error::Io(e).compat())))
                },
                Poll::Ready(Ok(n)) => {
                    buf.extend_from_slice(&read_buf[..n]);
                    if buf.is_empty() && n == 0 {
                        return Poll::Ready(None);
                    } else {
                        return Poll::Ready(Some(Ok(buf.into())));
                    }
                }
            }
        }
    }
}

async fn post<P>(path: P) -> Result<(), Error>
where
    P: AsRef<Path> + Send + Unpin + 'static,
{
    let source = File::open(path)
        .await.map_err(Error::Io)?;
    let wrapper = AsyncReadWrapper { inner: source };
    let mut res = Client::new()
        .post("https://httpbin.org/post")
        .body(Body::wrap_stream(wrapper))
        .send()
        .await.map_err(Error::Reqwest)?;

    println!("{}", res.status());

    let body = mem::replace(res.body_mut(), Decoder::empty());
    let body: Result<_, _> = body.try_concat().await;

    let mut body = Cursor::new(body.map_err(Error::Reqwest)?);
    io::copy(&mut body, &mut io::stdout()).map_err(Error::Io)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/LICENSE-APACHE");
    post(path).await
}