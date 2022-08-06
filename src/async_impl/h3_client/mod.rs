#![cfg(feature = "http3")]

mod pool;

use crate::async_impl::h3_client::pool::{Key, Pool, PoolClient};
use crate::error::{BoxError, Error, Kind};
use crate::{error, Body};
use futures_util::future;
use h3_quinn::Connection;
use http::{Request, Response};
use hyper::Body as HyperBody;
use log::debug;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

pub struct H3Builder {
    pool_idle_timeout: Option<Duration>,
    pool_max_idle_per_host: usize,
    local_addr: Option<IpAddr>,
}

impl Default for H3Builder {
    fn default() -> Self {
        Self {
            pool_idle_timeout: Some(Duration::from_secs(90)),
            pool_max_idle_per_host: usize::MAX,
            local_addr: None,
        }
    }
}

impl H3Builder {
    pub fn build(self, tls: rustls::ClientConfig) -> H3Client {
        let config = quinn::ClientConfig::new(Arc::new(tls));
        let socket_addr = match self.local_addr {
            Some(ip) => SocketAddr::new(ip, 0),
            None => "[::]:0".parse::<SocketAddr>().unwrap(),
        };

        let mut endpoint =
            quinn::Endpoint::client(socket_addr).expect("unable to create QUIC endpoint");
        endpoint.set_default_client_config(config);

        H3Client {
            endpoint,
            pool: Pool::new(self.pool_max_idle_per_host, self.pool_idle_timeout),
        }
    }

    pub fn set_pool_idle_timeout(&mut self, timeout: Option<Duration>) {
        self.pool_idle_timeout = timeout;
    }

    pub fn set_pool_max_idle_per_host(&mut self, max: usize) {
        self.pool_max_idle_per_host = max;
    }

    pub fn set_local_addr(&mut self, addr: Option<IpAddr>) {
        self.local_addr = addr;
    }
}

#[derive(Clone)]
pub struct H3Client {
    endpoint: quinn::Endpoint,
    pool: Pool,
    // TODO: Since resolution is perform internally in Hyper,
    // we have no way of leveraging that functionality here.
    // resolver: Box<dyn Resolve>,
}

impl H3Client {
    async fn get_pooled_client(&self, key: Key) -> Result<PoolClient, BoxError> {
        if let Some(client) = self.pool.try_pool(&key) {
            debug!("getting client from pool with key {:?}", key);
            return Ok(client);
        }

        let dest = pool::domain_as_uri(key.clone());
        let auth = dest
            .authority()
            .ok_or("destination must have a host")?
            .clone();
        let port = auth.port_u16().unwrap_or(443);
        let addr = tokio::net::lookup_host((auth.host(), port))
            .await?
            .next()
            .ok_or("dns found no addresses")?;

        let quinn_conn = Connection::new(self.endpoint.connect(addr, auth.host())?.await?);
        let (mut driver, tx) = h3::client::new(quinn_conn).await?;

        // TODO: What does poll_close() do?
        tokio::spawn(async move {
            future::poll_fn(|cx| driver.poll_close(cx)).await.unwrap();
        });

        let client = PoolClient::new(tx);
        self.pool.put(key, client.clone());
        Ok(client)
    }

    async fn send_request(
        self,
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
