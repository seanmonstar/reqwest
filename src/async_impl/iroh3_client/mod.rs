pub mod connect;
pub mod connection;
// pub mod endpoint;
pub mod pool;

use crate::async_impl::body::ResponseBody;
use crate::async_impl::iroh3_client::pool::{Key, Pool, PoolClient};
use crate::error::{BoxError, Error, Kind};
use crate::{error, Body};
use connect::Iroh3Connector;

use http::{Request, Response};
use log::trace;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use sync_wrapper::SyncWrapper;
use tower::Service;

#[derive(Clone)]
pub(crate) struct Iroh3Client {
    ticket: String,
    pool: Pool,
    connector: Iroh3Connector,
}

impl Iroh3Client {
    pub fn new(connector: Iroh3Connector, ticket: String, pool_timeout: Option<Duration>) -> Self {
        Iroh3Client {
            ticket,
            pool: Pool::new(pool_timeout),
            connector,
        }
    }

    async fn get_pooled_client(&mut self, key: Key) -> Result<PoolClient, BoxError> {
        if let Some(client) = self.pool.try_pool(&key) {
            trace!("getting client from pool with key {key:?}");
            return Ok(client);
        }

        trace!("did not find connection {key:?} in pool so connecting...");

        let ticket = key.clone();

        let lock = match self.pool.connecting(&key) {
            pool::Connecting::InProgress(waiter) => {
                trace!("connecting to {key:?} is already in progress, subscribing...");

                match waiter.receive().await {
                    Some(client) => return Ok(client),
                    None => return Err("failed to establish connection for HTTP/3 request".into()),
                }
            }
            pool::Connecting::Acquired(lock) => lock,
        };
        trace!("connecting to {key:?}...");
        let (driver, tx) = self.connector.connect(&ticket).await?;
        trace!("saving new pooled connection to {key:?}");
        Ok(self.pool.new_connection(lock, driver, tx))
    }

    async fn send_request(
        mut self,
        key: Key,
        req: Request<Body>,
    ) -> Result<Response<ResponseBody>, Error> {
        let mut pooled = match self.get_pooled_client(key).await {
            Ok(client) => client,
            Err(e) => return Err(error::request(e)),
        };
        pooled
            .send_request(req)
            .await
            .map_err(|e| Error::new(Kind::Request, Some(e)))
    }

    pub fn request(&self, req: Request<Body>) -> H3ResponseFuture {
        let pool_key = self.ticket.clone();
        H3ResponseFuture {
            inner: SyncWrapper::new(Box::pin(self.clone().send_request(pool_key, req))),
        }
    }
}

impl Service<Request<Body>> for Iroh3Client {
    type Response = Response<ResponseBody>;
    type Error = Error;
    type Future = H3ResponseFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        self.request(req)
    }
}

pub(crate) struct H3ResponseFuture {
    inner: SyncWrapper<Pin<Box<dyn Future<Output = Result<Response<ResponseBody>, Error>> + Send>>>,
}

impl Future for H3ResponseFuture {
    type Output = Result<Response<ResponseBody>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.get_mut().as_mut().poll(cx)
    }
}
