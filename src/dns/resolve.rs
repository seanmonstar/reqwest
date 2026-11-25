use hyper_util::client::legacy::connect::dns::Name as HyperName;
use tower_service::Service;

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::error::BoxError;

/// Alias for an `Iterator` trait object over `SocketAddr`.
pub type Addrs = Box<dyn Iterator<Item = SocketAddr> + Send>;

/// Alias for the `Future` type returned by a DNS resolver.
pub type Resolving = Pin<Box<dyn Future<Output = Result<Addrs, BoxError>> + Send>>;

/// Trait for customizing DNS resolution in reqwest.
pub trait Resolve: Send + Sync {
    /// Performs DNS resolution on a `Name`.
    /// The return type is a future containing an iterator of `SocketAddr`.
    ///
    /// It differs from `tower_service::Service<Name>` in several ways:
    ///  * It is assumed that `resolve` will always be ready to poll.
    ///  * It does not need a mutable reference to `self`.
    ///  * Since trait objects cannot make use of associated types, it requires
    ///    wrapping the returned `Future` and its contained `Iterator` with `Box`.
    ///
    /// Explicitly specified port in the URL will override any port in the resolved `SocketAddr`s.
    /// Otherwise, port `0` will be replaced by the conventional port for the given scheme (e.g. 80 for http).
    fn resolve(&self, name: Name) -> Resolving;
}

/// A name that must be resolved to addresses.
#[derive(Debug)]
pub struct Name(pub(super) HyperName);

/// A more general trait implemented for types implementing `Resolve`.
///
/// Unnameable, only exported to aid seeing what implements this.
pub trait IntoResolve {
    #[doc(hidden)]
    fn into_resolve(self) -> Arc<dyn Resolve>;
}

impl Name {
    /// View the name as a string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl FromStr for Name {
    type Err = sealed::InvalidNameError;

    fn from_str(host: &str) -> Result<Self, Self::Err> {
        HyperName::from_str(host)
            .map(Name)
            .map_err(|_| sealed::InvalidNameError { _ext: () })
    }
}

#[derive(Clone)]
pub(crate) struct DynResolver {
    resolver: Arc<dyn Resolve>,
}

impl DynResolver {
    pub(crate) fn new(resolver: Arc<dyn Resolve>) -> Self {
        Self { resolver }
    }

    #[cfg(feature = "socks")]
    pub(crate) fn gai() -> Self {
        Self::new(Arc::new(super::gai::GaiResolver::new()))
    }

    /// Resolve an HTTP host and port, not just a domain name.
    ///
    /// This does the same thing that hyper-util's HttpConnector does, before
    /// calling out to its underlying DNS resolver.
    #[cfg(feature = "socks")]
    pub(crate) async fn http_resolve(
        &self,
        target: &http::Uri,
    ) -> Result<impl Iterator<Item = std::net::SocketAddr>, BoxError> {
        let host = target.host().ok_or("missing host")?;
        let port = target
            .port_u16()
            .unwrap_or_else(|| match target.scheme_str() {
                Some("https") => 443,
                Some("socks4") | Some("socks4a") | Some("socks5") | Some("socks5h") => 1080,
                _ => 80,
            });

        let explicit_port = target.port().is_some();

        let addrs = self
            .resolver
            .resolve(host.parse()?)
            .await
            .map_err(crate::error::dns)?;

        Ok(addrs.map(move |mut addr| {
            if explicit_port || addr.port() == 0 {
                addr.set_port(port);
            }
            addr
        }))
    }
}

impl Service<HyperName> for DynResolver {
    type Response = Addrs;
    type Error = BoxError;
    type Future = Resolving;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: HyperName) -> Self::Future {
        let resolving = self.resolver.resolve(Name(name));
        // Tag resolution failures so `Error::is_dns` can recognize them once
        Box::pin(async move { resolving.await.map_err(crate::error::dns) })
    }
}

pub(crate) struct DnsResolverWithOverrides {
    dns_resolver: Arc<dyn Resolve>,
    overrides: Arc<HashMap<String, Vec<SocketAddr>>>,
}

impl DnsResolverWithOverrides {
    pub(crate) fn new(
        dns_resolver: Arc<dyn Resolve>,
        overrides: HashMap<String, Vec<SocketAddr>>,
    ) -> Self {
        DnsResolverWithOverrides {
            dns_resolver,
            overrides: Arc::new(overrides),
        }
    }
}

impl Resolve for DnsResolverWithOverrides {
    fn resolve(&self, name: Name) -> Resolving {
        match self.overrides.get(name.as_str()) {
            Some(dest) => {
                let addrs: Addrs = Box::new(dest.clone().into_iter());
                Box::pin(std::future::ready(Ok(addrs)))
            }
            None => self.dns_resolver.resolve(name),
        }
    }
}

/// A resolver that wraps another resolver and filters out any resolved IP addresses that are not globally reachable.
pub(crate) struct GlobalIpsOnlyResolver {
    dns_resolver: Arc<dyn Resolve>,
}

impl GlobalIpsOnlyResolver {
    pub(crate) fn new(dns_resolver: Arc<dyn Resolve>) -> Self {
        GlobalIpsOnlyResolver { dns_resolver }
    }
}

impl Resolve for GlobalIpsOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolving = self.dns_resolver.resolve(name);
        Box::pin(async move {
            let addrs = resolving.await?;
            let filtered = addrs
                .filter(|addr| is_global(&addr.ip()))
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return Err("destination is not a globally reachable IP".into());
            }
            Ok(Box::new(filtered.into_iter()) as Addrs)
        })
    }
}

/// Returns `true` if the given host string is a hostname or a globally reachable IP address.
/// Otherwise returns `false`.
pub(crate) fn is_hostname_or_global_ip_literal(host: &str) -> bool {
    // Strip the brackets around IPv6 literals.
    host.strip_prefix('[')
        .unwrap_or(host)
        .strip_suffix(']')
        .unwrap_or(host)
        .parse::<IpAddr>()
        .map_or(true, |ip| is_global(&ip))
}

/// Returns `true` if the address is globally reachable.
///
/// This is an inlined version of `IpAddr::is_global`, which currently requires nightly.
/// Once that is stabilized, we can use it directly.
pub(crate) fn is_global(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_global_v4(ip),
        IpAddr::V6(ip) => is_global_v6(ip),
    }
}

fn is_global_v4(ip: &Ipv4Addr) -> bool {
    !(ip.octets()[0] == 0 // "This network"
            || ip.is_private()
            || ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000 == 0b0100_0000) // is_shared
            || ip.is_loopback()
            || ip.is_link_local()
            // addresses reserved for future protocols (`192.0.0.0/24`)
            // .9 and .10 are documented as globally reachable so they're excluded
            || (
                ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 0
                && ip.octets()[3] != 9 && ip.octets()[3] != 10
            )
            || ip.is_documentation()
            || ip.octets()[0] == 198 && (ip.octets()[1] & 0xfe) == 18 // is_benchmarking
            || ip.octets()[0] & 240 == 240 && !ip.is_broadcast() // is_reserved
            || ip.is_broadcast())
}

fn is_global_v6(ip: &Ipv6Addr) -> bool {
    !(ip.is_unspecified()
            || ip.is_loopback()
            // IPv4-mapped Address (`::ffff:0:0/96`)
            || matches!(ip.segments(), [0, 0, 0, 0, 0, 0xffff, _, _])
            // IPv4-IPv6 Translat. (`64:ff9b:1::/48`)
            || matches!(ip.segments(), [0x64, 0xff9b, 1, _, _, _, _, _])
            // Discard-Only Address Block (`100::/64`)
            || matches!(ip.segments(), [0x100, 0, 0, 0, _, _, _, _])
            // IETF Protocol Assignments (`2001::/23`)
            || (matches!(ip.segments(), [0x2001, b, _, _, _, _, _, _] if b < 0x200)
                && !(
                    // Port Control Protocol Anycast (`2001:1::1`)
                    u128::from_be_bytes(ip.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0001
                    // Traversal Using Relays around NAT Anycast (`2001:1::2`)
                    || u128::from_be_bytes(ip.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0002
                    // AMT (`2001:3::/32`)
                    || matches!(ip.segments(), [0x2001, 3, _, _, _, _, _, _])
                    // AS112-v6 (`2001:4:112::/48`)
                    || matches!(ip.segments(), [0x2001, 4, 0x112, _, _, _, _, _])
                    // ORCHIDv2 (`2001:20::/28`)
                    // Drone Remote ID Protocol Entity Tags (DETs) Prefix (`2001:30::/28`)`
                    || matches!(ip.segments(), [0x2001, b, _, _, _, _, _, _] if b >= 0x20 && b <= 0x3F)
                ))
            // 6to4 (`2002::/16`) – it's not explicitly documented as globally reachable,
            // IANA says N/A.
            || matches!(ip.segments(), [0x2002, _, _, _, _, _, _, _])
            || matches!(ip.segments(), [0x2001, 0xdb8, ..] | [0x3fff, 0..=0x0fff, ..]) // is_documentation
            // Segment Routing (SRv6) SIDs (`5f00::/16`)
            || matches!(ip.segments(), [0x5f00, ..])
            || ip.is_unique_local()
            || ip.is_unicast_link_local())
}

impl IntoResolve for Arc<dyn Resolve> {
    fn into_resolve(self) -> Arc<dyn Resolve> {
        self
    }
}

impl<R> IntoResolve for Arc<R>
where
    R: Resolve + 'static,
{
    fn into_resolve(self) -> Arc<dyn Resolve> {
        self
    }
}

impl<R> IntoResolve for R
where
    R: Resolve + 'static,
{
    fn into_resolve(self) -> Arc<dyn Resolve> {
        Arc::new(self)
    }
}

mod sealed {
    use std::fmt;

    #[derive(Debug)]
    pub struct InvalidNameError {
        pub(super) _ext: (),
    }

    impl fmt::Display for InvalidNameError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("invalid DNS name")
        }
    }

    impl std::error::Error for InvalidNameError {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn ipv4_global() {
        assert!(is_global(&ip("1.1.1.1")));
        assert!(is_global(&ip("8.8.8.8")));
        assert!(is_global(&ip("192.0.0.9"))); // PCP anycast, globally reachable
    }

    #[test]
    fn ipv4_not_global() {
        assert!(!is_global(&ip("0.0.0.0")));
        assert!(!is_global(&ip("10.0.0.1"))); // private
        assert!(!is_global(&ip("172.16.5.4"))); // private
        assert!(!is_global(&ip("192.168.1.1"))); // private
        assert!(!is_global(&ip("100.64.0.1"))); // shared (CGNAT)
        assert!(!is_global(&ip("127.0.0.1"))); // loopback
        assert!(!is_global(&ip("169.254.1.1"))); // link-local
        assert!(!is_global(&ip("169.254.169.254"))); // cloud metadata endpoint
        assert!(!is_global(&ip("192.0.2.1"))); // documentation
        assert!(!is_global(&ip("198.18.0.1"))); // benchmarking
        assert!(!is_global(&ip("240.0.0.1"))); // reserved
        assert!(!is_global(&ip("255.255.255.255"))); // broadcast
    }

    #[test]
    fn ipv6_global() {
        assert!(is_global(&ip("2606:4700:4700::1111")));
        assert!(is_global(&ip("2001:1::1"))); // PCP anycast
    }

    #[test]
    fn ipv6_not_global() {
        assert!(!is_global(&ip("::"))); // unspecified
        assert!(!is_global(&ip("::1"))); // loopback
        assert!(!is_global(&ip("::ffff:127.0.0.1"))); // IPv4-mapped
        assert!(!is_global(&ip("fc00::1"))); // unique local
        assert!(!is_global(&ip("fe80::1"))); // link-local
        assert!(!is_global(&ip("2001:db8::1"))); // documentation
    }
}
