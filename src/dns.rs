use std::{io, vec};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, Once};

use futures::{future, Future};
use hyper::client::connect::dns as hyper_dns;
use tokio;
use trust_dns_resolver::{system_conf, AsyncResolver, BackgroundLookupIp};

// If instead the type were just `AsyncResolver`, it breaks the default recursion limit
// for the compiler to determine if `reqwest::Client` is `Send`. This is because `AsyncResolver`
// has **a lot** of internal generic types that pushes us over the limit.
//
// "Erasing" the internal resolver type saves us from this limit.
type ErasedResolver = Box<dyn Fn(hyper_dns::Name) -> BackgroundLookupIp + Send + Sync>;
type Background = Box<dyn Future<Item=(), Error=()> + Send>;

#[derive(Clone)]
pub(crate) struct TrustDnsResolver {
    inner: Arc<Inner>,
}

struct Inner {
    background: Mutex<Option<Background>>,
    once: Once,
    resolver: ErasedResolver,
}

impl TrustDnsResolver {
    pub(crate) fn new() -> io::Result<Self> {
        let (conf, opts) = system_conf::read_system_conf()?;
        let (resolver, bg) = AsyncResolver::new(conf, opts);

        let resolver: ErasedResolver = Box::new(move |name| {
            resolver.lookup_ip(name.as_str())
        });
        let background = Mutex::new(Some(Box::new(bg) as Background));
        let once = Once::new();

        Ok(TrustDnsResolver {
            inner: Arc::new(Inner {
                background,
                once,
                resolver,
            }),
        })
    }
}

impl hyper_dns::Resolve for TrustDnsResolver {
    type Addrs = vec::IntoIter<IpAddr>;
    type Future = Box<dyn Future<Item=Self::Addrs, Error=io::Error> + Send>;

    fn resolve(&self, name: hyper_dns::Name) -> Self::Future {
        let inner = self.inner.clone();
        Box::new(future::lazy(move || {
            inner.once.call_once(|| {
                // The `bg` (background) future needs to be spawned onto an executor,
                // but a `reqwest::Client` may be constructed before an executor is ready.
                // So, the `bg` future cannot be spawned *until* the executor starts to
                // `poll` this future.
                let bg = inner
                    .background
                    .lock()
                    .expect("resolver background lock")
                    .take()
                    .expect("background only taken once");

                tokio::spawn(bg);
            });

            (inner.resolver)(name)
                .map(|lookup| {
                    lookup
                        .iter()
                        .collect::<Vec<_>>()
                        .into_iter()
                })
                .map_err(|err| {
                    io::Error::new(io::ErrorKind::Other, err.to_string())
                })
        }))
    }
}

