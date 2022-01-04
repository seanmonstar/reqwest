//! DNS client resolution options
//!
//! ```
//! # use reqwest::Error;
//! #
//! # async fn run() -> Result<(), Error> {
//! use reqwest::dns::{ResolverConfig, ResolverOpts};
//! use std::time::Duration;
//!
//! let config = ResolverConfig::default();
//! let opts = ResolverOpts {
//!     negative_max_ttl: Some(Duration::from_secs(60)),
//!     positive_max_ttl: Some(Duration::from_secs(60 * 60)),
//!     ..Default::default()
//! };
//! let client = reqwest::Client::builder()
//!     .dns_config(config, opts)
//!     .build()
//!     .expect("unable to build reqwest client");
//!
//! let res = client.get("https://doc.rust-lang.org").send().await?;
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{self, Poll};

use hyper::client::connect::dns as hyper_dns;
use hyper::service::Service;
use tokio::sync::Mutex;
use trust_dns_resolver::{
    lookup_ip::LookupIpIntoIter, system_conf, AsyncResolver, TokioConnection,
    TokioConnectionProvider, TokioHandle,
};

use crate::error::BoxError;

// make them accessible for consuming applications
pub use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};

type SharedResolver = Arc<AsyncResolver<TokioConnection, TokioConnectionProvider>>;

lazy_static! {
    static ref SYSTEM_CONF: io::Result<(ResolverConfig, ResolverOpts)> =
        system_conf::read_system_conf().map_err(io::Error::from);
}

#[derive(Clone)]
pub(crate) struct TrustDnsResolver {
    state: Arc<Mutex<State>>,
}

pub(crate) struct SocketAddrs {
    iter: LookupIpIntoIter,
}

enum State {
    Init(Option<(ResolverConfig, ResolverOpts)>),
    Ready(SharedResolver),
}

impl State {
    async fn get_resolver(&mut self) -> Result<SharedResolver, BoxError> {
        let resolver = match self {
            State::Init(user_config) => {
                let (config, opts) = if let Some(user) = &*user_config {
                    // TODO this clone is pointless, hope for copy elision
                    user.clone()
                } else {
                    let system: &(ResolverConfig, ResolverOpts) = SYSTEM_CONF
                        .as_ref()
                        .expect("can't construct TrustDnsResolver if SYSTEM_CONF is error");
                    system.clone()
                };
                let resolver = Arc::new(AsyncResolver::new(config, opts, TokioHandle)?);
                *self = State::Ready(resolver.clone());
                resolver
            }
            State::Ready(resolver) => resolver.clone(),
        };
        Ok(resolver)
    }
}

impl TrustDnsResolver {
    pub(crate) fn new(user_config: Option<(ResolverConfig, ResolverOpts)>) -> io::Result<Self> {
        if user_config.is_none() {
            // only touch and load the system config if no configuration is supplied so users get an early error
            SYSTEM_CONF.as_ref().map_err(|e| {
                io::Error::new(e.kind(), format!("error reading DNS system conf: {}", e))
            })?;
        }

        // At this stage, we might not have been called in the context of a
        // Tokio Runtime, so we must delay the actual construction of the
        // resolver.
        Ok(TrustDnsResolver {
            state: Arc::new(Mutex::new(State::Init(user_config))),
            //runtime_config,
        })
    }
}

impl Service<hyper_dns::Name> for TrustDnsResolver {
    type Response = SocketAddrs;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: hyper_dns::Name) -> Self::Future {
        let resolver = self.clone();
        Box::pin(async move {
            let mut lock = resolver.state.lock().await;

            let resolver = lock.get_resolver().await?;

            // Don't keep lock once the resolver is constructed, otherwise
            // only one lookup could be done at a time.
            drop(lock);

            let lookup = resolver.lookup_ip(name.as_str()).await?;
            Ok(SocketAddrs {
                iter: lookup.into_iter(),
            })
        })
    }
}

impl Iterator for SocketAddrs {
    type Item = SocketAddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|ip_addr| SocketAddr::new(ip_addr, 0))
    }
}
