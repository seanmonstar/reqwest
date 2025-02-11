//! DNS resolution

pub use gai::GaiResolver;
#[cfg(feature = "hickory-dns")]
pub use hickory::HickoryDnsResolver;
pub use resolve::{Addrs, Name, Resolve, Resolving};
pub(crate) use resolve::{DnsResolverWithOverrides, DynResolver};

pub(crate) mod gai;
#[cfg(feature = "hickory-dns")]
pub(crate) mod hickory;
pub(crate) mod resolve;
