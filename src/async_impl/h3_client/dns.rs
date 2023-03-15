use crate::dns::DnsResolverWithOverrides;
#[cfg(feature = "trust-dns")]
use crate::dns::trust_dns::TrustDnsResolver;
use core::task;
use hyper::client::connect::dns::{GaiResolver, Name};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use tower_service::Service;

#[derive(Clone)]
pub(crate) enum Resolver {
    Gai(GaiResolver),
    GaiWithDnsOverrides(DnsResolverWithOverrides),
    #[cfg(feature = "trust-dns")]
    TrustDns(TrustDnsResolver),
    #[cfg(feature = "trust-dns")]
    TrustDnsWithOverrides(DnsResolverWithOverrides),
}

impl Resolver {
    pub fn new_gai() -> Self {
        Self::Gai(GaiResolver::new())
    }

    pub fn new_gai_with_overrides(overrides: HashMap<String, Vec<SocketAddr>>) -> Self {
        Self::GaiWithDnsOverrides(DnsResolverWithOverrides::new(Arc::new(GaiResolver::new()), overrides))
    }

    #[cfg(feature = "trust-dns")]
    pub fn new_trust_dns() -> crate::Result<Self> {
        TrustDnsResolver::new()
            .map(Self::TrustDns)
            .map_err(crate::error::builder)
    }

    #[cfg(feature = "trust-dns")]
    pub fn new_trust_dns_with_overrides(
        overrides: HashMap<String, Vec<SocketAddr>>,
    ) -> crate::Result<Self> {
        TrustDnsResolver::new()
            .map(|trust_resolver| DnsResolverWithOverrides::new(Arc::new(trust_resolver), overrides))
            .map(Self::TrustDnsWithOverrides)
            .map_err(crate::error::builder)
    }

    pub async fn resolve(&mut self, server_name: &str) -> Vec<SocketAddr> {
        let res: Vec<SocketAddr> = match self {
            Self::Gai(resolver) => resolve(resolver, Name::from_str(server_name).unwrap())
                .await
                .unwrap()
                .collect(),
            Self::GaiWithDnsOverrides(resolver) => {
                resolve(resolver, Name::from_str(server_name).unwrap())
                    .await
                    .unwrap()
                    .collect()
            }
            #[cfg(feature = "trust-dns")]
            Self::TrustDns(resolver) => resolve(resolver, Name::from_str(server_name).unwrap())
                .await
                .unwrap()
                .collect(),
            #[cfg(feature = "trust-dns")]
            Self::TrustDnsWithOverrides(resolver) => {
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
