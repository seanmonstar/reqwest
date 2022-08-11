#![cfg(feature = "http3")]

pub(crate) mod connect;
pub(crate) mod dns;
mod pool;

use crate::async_impl::h3_client::pool::{Key, Pool, PoolClient};
use crate::error::{BoxError, Error, Kind};
use crate::{error, Body};
use connect::H3Connector;
use futures_util::future;
use http::{Request, Response};
use hyper::Body as HyperBody;
use log::debug;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

pub(crate) struct H3Builder {
    pool_idle_timeout: Option<Duration>,
    pool_max_idle_per_host: usize,
}

impl Default for H3Builder {
    fn default() -> Self {
        Self {
            pool_idle_timeout: Some(Duration::from_secs(90)),
            pool_max_idle_per_host: usize::MAX,
        }
    }
}

impl H3Builder {
    pub fn build(self, connector: H3Connector) -> H3Client {
        H3Client {
            pool: Pool::new(self.pool_max_idle_per_host, self.pool_idle_timeout),
            connector,
        }
    }

    pub fn set_pool_idle_timeout(&mut self, timeout: Option<Duration>) {
        self.pool_idle_timeout = timeout;
    }

    pub fn set_pool_max_idle_per_host(&mut self, max: usize) {
        self.pool_max_idle_per_host = max;
    }
}

#[derive(Clone)]
pub struct H3Client {
    pool: Pool,
    connector: H3Connector,
}

impl H3Client {
    async fn get_pooled_client(&mut self, key: Key) -> Result<PoolClient, BoxError> {
        if let Some(client) = self.pool.try_pool(&key) {
            debug!("getting client from pool with key {:?}", key);
            return Ok(client);
        }

        let dest = pool::domain_as_uri(key.clone());
        let tx = self.connector.connect(dest).await?;
        let client = PoolClient::new(tx);
        self.pool.put(key, client.clone());
        Ok(client)
    }

    async fn send_request(
        mut self,
        key: Key,
        req: Request<Body>,
    ) -> Result<Response<HyperBody>, Error> {
        let mut pooled = match self.get_pooled_client(key).await {
            Ok(client) => client,
            Err(e) => return Err(error::request(e)),
        };
        pooled
            .send_request(req)
            .await
            .map_err(|e| Error::new(Kind::Request, Some(e)))
    }

    pub fn request(&self, mut req: Request<Body>) -> H3ResponseFuture {
        let pool_key = match pool::extract_domain(req.uri_mut()) {
            Ok(s) => s,
            Err(e) => {
                return H3ResponseFuture {
                    inner: Box::pin(future::err(e)),
                }
            }
        };
        H3ResponseFuture {
            inner: Box::pin(self.clone().send_request(pool_key, req)),
        }
    }
}

pub struct H3ResponseFuture {
    inner: Pin<Box<dyn Future<Output = Result<Response<HyperBody>, Error>> + Send>>,
}

impl Future for H3ResponseFuture {
    type Output = Result<Response<HyperBody>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}
