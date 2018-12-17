use std::io;
use std::sync::{Arc, Mutex};

use futures::{future, Future};
use hyper::client::connect::dns as hyper_dns;
use trust_dns_resolver::AsyncResolver;

// If instead the type were just `AsyncResolver`, it breaks the default recursion limit
// for the compiler to determine if `reqwest::Client` is `Send`. This is because `AsyncResolver`
// has **a lot** of internal generic types that pushes us over the limit.
//
// "Erasing" the internal resolver type saves us from this limit.
type ErasedResolver = Box<Fn(hyper_dns::Name) -> ::trust_dns_resolver::BackgroundLookupIp + Send + Sync>;

#[derive(Clone)]
pub(crate) struct TrustDnsResolver {
    inner: Arc<Mutex<Option<ErasedResolver>>>,
}

impl TrustDnsResolver {
    pub(crate) fn new() -> Self {
        TrustDnsResolver {
            inner: Arc::new(Mutex::new(None)),
        }
    }
}

impl hyper_dns::Resolve for TrustDnsResolver {
    type Addrs = ::std::vec::IntoIter<::std::net::IpAddr>;
    type Future = Box<Future<Item=Self::Addrs, Error=io::Error> + Send>;

    fn resolve(&self, name: hyper_dns::Name) -> Self::Future {
        let inner = self.inner.clone();
        Box::new(future::lazy(move || {
            let mut inner = inner.lock().expect("lock resolver");
            if inner.is_none() {
                // The `bg` (background) future needs to be spawned onto an executor,
                // but a `reqwest::Client` may be constructed before an executor is ready.
                // So, the `bg` future cannot be spawned *until* the executor starts to
                // `poll` this future.
                match AsyncResolver::from_system_conf() {
                    Ok((resolver, bg)) => {
                        ::tokio::spawn(bg);
                        *inner = Some(Box::new(move |name| {
                            resolver.lookup_ip(name.as_str())
                        }));
                    },
                    Err(err) => {
                        return future::Either::A(
                            future::err(io::Error::new(io::ErrorKind::Other, err.to_string()))
                        );
                    }
                }
            }

            future::Either::B((inner
                .as_mut()
                .expect("resolver is set"))(name)
                //.lookup_ip(name.as_str())
                .map(|lookup| {
                    lookup
                        .iter()
                        .collect::<Vec<_>>()
                        .into_iter()
                })
                .map_err(|err| {
                    io::Error::new(io::ErrorKind::Other, err.to_string())
                }))
        }))
    }
}


