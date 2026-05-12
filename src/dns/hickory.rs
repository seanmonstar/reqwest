//! DNS resolution via the [hickory-resolver](https://github.com/hickory-dns/hickory-dns) crate

use hickory_resolver::{
    config::{LookupIpStrategy, ResolverConfig},
    net::{runtime::TokioRuntimeProvider, NetError},
    TokioResolver,
};
use once_cell::sync::OnceCell;

use std::net::SocketAddr;
use std::sync::Arc;

use super::{Addrs, Name, Resolve, Resolving};

/// Wrapper around an `AsyncResolver`, which implements the `Resolve` trait.
#[derive(Debug, Default, Clone)]
pub(crate) struct HickoryDnsResolver {
    /// Since we might not have been called in the context of a
    /// Tokio Runtime in initialization, so we must delay the actual
    /// construction of the resolver.
    state: Arc<OnceCell<TokioResolver>>,
}

struct SocketAddrs {
    iter: std::vec::IntoIter<SocketAddr>,
}

impl Resolve for HickoryDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.clone();
        Box::pin(async move {
            let resolver = resolver.state.get_or_try_init(new_resolver)?;

            let lookup = resolver.lookup_ip(name.as_str()).await?;
            let addrs = lookup
                .iter()
                .map(|ip| SocketAddr::new(ip, 0))
                .collect::<Vec<_>>();
            let addrs: Addrs = Box::new(SocketAddrs {
                iter: addrs.into_iter(),
            });
            Ok(addrs)
        })
    }
}

impl Iterator for SocketAddrs {
    type Item = SocketAddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

/// Create a new resolver with the default configuration,
/// which reads from `/etc/resolve.conf`. If reading `/etc/resolv.conf` fails,
/// it fallbacks to hickory_resolver's default config.
/// The options are overridden to look up for both IPv4 and IPv6 addresses
/// to work with "happy eyeballs" algorithm.
fn new_resolver() -> Result<TokioResolver, NetError> {
    let mut builder = TokioResolver::builder_tokio().unwrap_or_else(|err| {
        log::debug!(
            "hickory-dns: failed to load system DNS configuration; falling back to hickory_resolver defaults: {:?}",
            err
        );
        TokioResolver::builder_with_config(
            ResolverConfig::default(),
            TokioRuntimeProvider::default(),
        )
    });
    builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
    builder.build()
}
