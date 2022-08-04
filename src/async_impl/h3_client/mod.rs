mod pool;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use http::{Request, Response};
use crate::error::{BoxError, Error};
use hyper::Body;
use futures_util::future;
use h3_quinn::Connection;
use crate::async_impl::h3_client::pool::{Key, Pool, PoolClient};

#[derive(Clone)]
pub struct H3Client {
    endpoint: quinn::Endpoint,
    pool: Pool,
    // TODO: Since resolution is perform internally in Hyper,
    // we have no way of leveraging that functionality here.
    // resolver: Box<dyn Resolve>,
}

impl H3Client {
    pub fn new(mut tls: rustls::ClientConfig) -> Self {
        tls.enable_early_data = true;
        let config = quinn::ClientConfig::new(Arc::new(tls));
        let mut endpoint = quinn::Endpoint::client("[::]:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(config);
        Self {
            endpoint,
            pool: Pool::new()
        }
    }

    async fn get_pooled_client(&self, key: Key) -> Result<PoolClient, BoxError> {
        if let Some(client) = self.pool.try_pool(&key) {
            eprintln!("found a client for {:?} in the pool", key);
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

        let quinn_conn = Connection::new(
            self.endpoint.connect(addr, auth.host())?.await?
        );
        let (mut driver, tx) = h3::client::new(quinn_conn).await?;

        // TODO: What does poll_close() do?
        tokio::spawn(async move {
            future::poll_fn(|cx| driver.poll_close(cx)).await.unwrap();
        });

        let client = PoolClient::new(tx);
        self.pool.put(key, client.clone());
        Ok(client)
    }

    async fn send_request(self, key: Key, req: Request<()>) -> Result<Response<Body>, Error> {
        eprintln!("Trying http3 ...");
        let mut pooled = match self.get_pooled_client(key).await {
            Ok(client) => client,
            Err(_) => panic!("failed to get pooled client")
        };
        pooled.send_request(req).await
    }

    pub fn request(&self, mut req: Request<()>) -> H3ResponseFuture {
        let pool_key = match pool::extract_domain(req.uri_mut(), false) {
            Ok(s) => s,
            Err(_) => panic!("invalid pool key")
        };
        H3ResponseFuture{inner: Box::pin(self.clone().send_request(pool_key, req))}
    }
}

pub struct H3ResponseFuture {
    inner: Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>,
}

impl Future for H3ResponseFuture {
    type Output = Result<Response<Body>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}
