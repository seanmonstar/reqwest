#![cfg(not(target_arch = "wasm32"))]
use std::convert::Infallible;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use hyper_util::rt::TokioIo;
use tokio::net::UnixListener;
use tokio::sync::oneshot;

use tokio::runtime;

pub struct Server {
    panic_rx: std_mpsc::Receiver<()>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        if !std::thread::panicking() {
            self.panic_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("test server should not panic");
        }
    }
}

#[allow(unused)]
pub fn http<F, Fut>(socket: PathBuf, func: F) -> Server
where
    F: Fn(http::Request<hyper::body::Incoming>) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = http::Response<String>> + Send + 'static,
{
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (panic_tx, panic_rx) = std_mpsc::channel();

    let tname = format!(
        "test({})-support-server",
        thread::current().name().unwrap_or("<unknown>")
    );
    thread::Builder::new()
    .name(tname)
    .spawn(move || {
        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("new rt");

        let srv = rt.block_on(async move {
            let listener = UnixListener::bind(socket).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);

            let srv = hyper::service::service_fn(|req| {
                    let func = func.clone();
                    let fut = func(req);
                    async move { Ok::<_, Infallible>(fut.await) }
            });
            let mut conn = hyper::server::conn::http1::Builder::new()
                .keep_alive(false)
                .serve_connection(io, srv);
            let mut conn = Pin::new(&mut conn);
            tokio::select! {
                res = conn.as_mut() => {
                    res.unwrap();
                }
                _ = shutdown_rx => {
                    conn.as_mut().graceful_shutdown();
                }
            }
            let _ = panic_tx.send(());
        });
    })
    .unwrap();

    Server {
        panic_rx,
        shutdown_tx: Some(shutdown_tx),
    }
}
