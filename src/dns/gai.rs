use futures_util::future::FutureExt;
use hyper_util::client::legacy::connect::dns::GaiResolver as HyperGaiResolver;
use tower_service::Service;

use crate::dns::{Addrs, Name, Resolve, Resolving};
use crate::error::BoxError;

/// A resolver using blocking `getaddrinfo` calls in a threadpool.
///
/// Based on [`hyper_util`]s [`GaiResolver`](hyper_util::client::legacy::connect::dns::GaiResolver).
#[derive(Debug)]
pub struct GaiResolver(HyperGaiResolver);

impl GaiResolver {
    /// Construct a new [`GaiResolver`].
    pub fn new() -> Self {
        Self(HyperGaiResolver::new())
    }
}

impl Default for GaiResolver {
    fn default() -> Self {
        GaiResolver::new()
    }
}

impl Resolve for GaiResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let this = &mut self.0.clone();
        Box::pin(this.call(name.0).map(|result| {
            result
                .map(|addrs| -> Addrs { Box::new(addrs) })
                .map_err(|err| -> BoxError { Box::new(err) })
        }))
    }
}
