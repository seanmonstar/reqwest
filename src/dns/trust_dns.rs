//! DNS resolution via the [trust_dns_resolver](https://github.com/bluejekyll/trust-dns) crate

use futures::executor::block_on;
use hyper::client::connect::dns::Name;
use once_cell::sync::OnceCell;
pub use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::{lookup_ip::LookupIpIntoIter, system_conf, TokioAsyncResolver};

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use super::{Addrs, Resolve, Resolving};


use crate::error::BoxError;

type SharedResolver = Arc<AsyncResolver<TokioConnection, TokioConnectionProvider>>;

lazy_static! {
    static ref SYSTEM_CONF: Mutex<Lazy<io::Result<(ResolverConfig, ResolverOpts)>>> = {
        Mutex::new(Lazy::new(|| {
            system_conf::read_system_conf().map_err(io::Error::from)
        }))
    };
}

pub async fn reinitialize_system_conf() {
    let mut system_conf_lock = SYSTEM_CONF.lock().await;
    *system_conf_lock = Lazy::new(|| system_conf::read_system_conf().map_err(io::Error::from));
}

/// Wrapper around an `AsyncResolver`, which implements the `Resolve` trait.
#[derive(Debug, Default, Clone)]
pub(crate) struct TrustDnsResolver {
    /// Since we might not have been called in the context of a
    /// Tokio Runtime in initialization, so we must delay the actual
    /// construction of the resolver.
    state: Arc<OnceCell<TokioAsyncResolver>>,
}

struct SocketAddrs {
    iter: LookupIpIntoIter,
}

#[derive(Debug)]
enum State {
    Init,
    Ready(SharedResolver),
}

impl TrustDnsResolver {
    /// Create a new resolver with the default configuration,
    /// which reads from `/etc/resolve.conf`.
    pub fn new() -> io::Result<Self> {
        block_on(async {
            SYSTEM_CONF
                .lock()
                .await
                .as_ref()
                .map_err(|e| io::Error::new(e.kind(), format!("error reading DNS system conf: {}", e)))
                .unwrap();
        });
        // At this stage, we might not have been called in the context of a
        // Tokio Runtime, so we must delay the actual construction of the
        // resolver.
        Ok(TrustDnsResolver {
            state: Arc::new(Mutex::new(State::Init)),
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

async fn new_resolver() -> Result<SharedResolver, BoxError> {
    let (config, opts) = SYSTEM_CONF
        .lock()
        .await
        .as_ref()
        .unwrap()
        .clone();
    new_resolver_with_config(config, opts)
}

fn new_resolver_with_config(
    config: ResolverConfig,
    opts: ResolverOpts,
) -> Result<SharedResolver, BoxError> {
    let resolver = AsyncResolver::new(config, opts, TokioHandle)?;
    Ok(Arc::new(resolver))
}
