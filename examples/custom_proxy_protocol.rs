use std::{error::Error, io::Write, pin::pin};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

use async_trait::async_trait;
use http::Uri;
use reqwest::{AsyncStreamWrapper, Client, CustomProxyProtocol, Proxy};

#[tokio::main]
async fn main() {
    let proxy: Box<dyn CustomProxyProtocol> = Box::new(Example());
    let client = Client::builder()
        .proxy(Proxy::all(proxy).unwrap())
        .http1_only()
        .build()
        .unwrap();
    let mut response = client
        .get("http://www.hal.ipc.i.u-tokyo.ac.jp/~nakada/prog2015/alice.txt")
        .send()
        .await
        .unwrap();

    let mut stdout = std::io::stdout();
    while let Some(chunk) = response.chunk().await.unwrap() {
        stdout.write_all(&chunk).unwrap();
    }
    stdout.flush().unwrap();
}

#[derive(Clone)]
struct Example();
#[async_trait]
impl CustomProxyProtocol for Example {
    async fn connect(
        &self,
        dst: Uri,
    ) -> Result<AsyncStreamWrapper, Box<dyn Error + Send + Sync + 'static>> {
        let host = dst.host().ok_or("host is None")?;
        let port = match (dst.scheme_str(), dst.port_u16()) {
            (_, Some(p)) => p,
            (Some("http"), None) => 80,
            (Some("https"), None) => 443,
            _ => return Err("scheme is unknown and port is None.".into()),
        };
        eprintln!("Connecting to {}:{}", host, port);
        Ok(AsyncStreamWrapper::new(
            WrapStream(TcpStream::connect(format!("{}:{}", host, port)).await?),
            false,
        ))
    }
}

struct WrapStream<RW>(RW)
where
    RW: AsyncRead + AsyncWrite + Send + Unpin + 'static;
impl<RW> AsyncRead for WrapStream<RW>
where
    RW: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        eprintln!("read");
        pin!(&mut self.0).poll_read(cx, buf)
    }
}
impl<RW> AsyncWrite for WrapStream<RW>
where
    RW: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        eprintln!("write");
        std::io::stderr().write_all(buf).unwrap();
        pin!(&mut self.0).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        eprintln!("flush");
        pin!(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        eprintln!("shutdown");
        pin!(&mut self.0).poll_shutdown(cx)
    }
}
