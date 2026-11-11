//! DNS resolution

pub(crate) use resolve::{
    is_hostname_or_global_ip_literal, DnsResolverWithOverrides, DynResolver, GlobalIpsOnlyResolver,
};
pub use resolve::{Addrs, Name, Resolve, Resolving};

#[cfg(docsrs)]
pub use resolve::IntoResolve;

pub(crate) mod gai;
#[cfg(feature = "hickory-dns")]
pub(crate) mod hickory;
pub(crate) mod resolve;
