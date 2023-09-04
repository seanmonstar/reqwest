//! DNS resolution via the [trust_dns_resolver](https://github.com/bluejekyll/trust-dns) crate

use hyper::client::connect::dns::Name;
use once_cell::sync::OnceCell;
pub use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::{lookup_ip::LookupIpIntoIter, system_conf, TokioAsyncResolver};

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use super::{Addrs, Resolve, Resolving};

/// Wrapper around an `AsyncResolver`, which implements the `Resolve` trait.
#[derive(Debug, Clone)]
pub(crate) struct TrustDnsResolver {
    state: Arc<OnceCell<TokioAsyncResolver>>,
}

struct SocketAddrs {
    iter: LookupIpIntoIter,
}

impl TrustDnsResolver {
    /// Create a new resolver with the default configuration,
    /// which reads from `/etc/resolve.conf`.
    pub fn new() -> io::Result<Self> {
        // At this stage, we might not have been called in the context of a
        // Tokio Runtime, so we must delay the actual construction of the
        // resolver.
        Ok(TrustDnsResolver {
            state: Arc::new(OnceCell::new()),
        })
    }
}

impl Resolve for TrustDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.clone();
        Box::pin(async move {
            let resolver = resolver.state.get_or_try_init(new_resolver)?;

            let lookup = resolver.lookup_ip(name.as_str()).await?;
            let addrs: Addrs = Box::new(SocketAddrs {
                iter: lookup.into_iter(),
            });
            Ok(addrs)
        })
    }
}

impl Iterator for SocketAddrs {
    type Item = SocketAddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|ip_addr| SocketAddr::new(ip_addr, 0))
    }
}

fn new_resolver() -> io::Result<TokioAsyncResolver> {
    let (config, opts) = system_conf::read_system_conf()
        .map_err(|e| io::Error::new(e.kind(), format!("error reading DNS system conf: {}", e)))?;
    Ok(TokioAsyncResolver::tokio(config, opts))
}
