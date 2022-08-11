use crate::connect::DnsResolverWithOverrides;
use core::task;
use hyper::client::connect::dns::{GaiResolver, Name};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::str::FromStr;
use std::task::Poll;
use tower_service::Service;

#[derive(Clone)]
pub(crate) enum Resolver {
    Gai(GaiResolver),
    GaiWithDnsOverrides(DnsResolverWithOverrides<GaiResolver>),
}

impl Resolver {
    pub fn new_gai() -> Self {
        Resolver::Gai(GaiResolver::new())
    }

    pub fn new_gai_with_overrides(overrides: HashMap<String, SocketAddr>) -> Self {
        Resolver::GaiWithDnsOverrides(DnsResolverWithOverrides::new(GaiResolver::new(), overrides))
    }

    pub async fn resolve(&mut self, server_name: &str) -> Vec<SocketAddr> {
        let res: Vec<SocketAddr> = match self {
            Resolver::Gai(resolver) => resolve(resolver, Name::from_str(server_name).unwrap())
                .await
                .unwrap()
                .collect(),
            Resolver::GaiWithDnsOverrides(resolver) => {
                resolve(resolver, Name::from_str(server_name).unwrap())
                    .await
                    .unwrap()
                    .collect()
            }
        };
        res
    }
}

// Trait from hyper to implement DNS resolution for HTTP/3 client.
pub trait Resolve {
    type Addrs: Iterator<Item = SocketAddr>;
    type Error: Into<Box<dyn std::error::Error + Send + Sync>>;
    type Future: Future<Output = Result<Self::Addrs, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>>;
    fn resolve(&mut self, name: Name) -> Self::Future;
}

impl<S> Resolve for S
where
    S: Service<Name>,
    S::Response: Iterator<Item = SocketAddr>,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Addrs = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Service::poll_ready(self, cx)
    }

    fn resolve(&mut self, name: Name) -> Self::Future {
        Service::call(self, name)
    }
}

pub(super) async fn resolve<R>(resolver: &mut R, name: Name) -> Result<R::Addrs, R::Error>
where
    R: Resolve,
{
    futures_util::future::poll_fn(|cx| resolver.poll_ready(cx)).await?;
    resolver.resolve(name).await
}
