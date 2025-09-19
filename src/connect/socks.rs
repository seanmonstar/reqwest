use tower_service::Service;

use http::uri::Scheme;
use http::Uri;
use hyper_util::client::legacy::connect::proxy::{SocksV4, SocksV5};
use tokio::net::TcpStream;

use super::BoxError;
use crate::proxy::Intercepted;

pub(super) enum DnsResolve {
    Local,
    Proxy,
}

#[derive(Debug)]
pub(super) enum SocksProxyError {
    SocksNoHostInUrl,
    SocksLocalResolve(BoxError),
    SocksConnect(BoxError),
}

pub(super) async fn connect(
    proxy: Intercepted,
    dst: Uri,
    dns_mode: DnsResolve,
    resolver: &crate::dns::DynResolver,
    http_connector: &mut crate::connect::HttpConnector,
) -> Result<TcpStream, SocksProxyError> {
    let https = dst.scheme() == Some(&Scheme::HTTPS);
    let original_host = dst.host().ok_or(SocksProxyError::SocksNoHostInUrl)?;
    let mut host = original_host.to_owned();
    let port = match dst.port() {
        Some(p) => p.as_u16(),
        None if https => 443u16,
        _ => 80u16,
    };

    if let DnsResolve::Local = dns_mode {
        let maybe_new_target = resolver
            .http_resolve(&dst)
            .await
            .map_err(SocksProxyError::SocksLocalResolve)?
            .next();
        if let Some(new_target) = maybe_new_target {
            log::trace!("socks local dns resolved {new_target:?}");
            // If the resolved IP is IPv6, wrap it in brackets for URI formatting
            let ip = new_target.ip();
            if ip.is_ipv6() {
                host = format!("[{}]", ip);
            } else {
                host = ip.to_string();
            }
        }
    }

    let proxy_uri = proxy.uri().clone();
    // Build a Uri for the destination
    let dst_uri = format!(
        "{}://{}:{}",
        if https { "https" } else { "http" },
        host,
        port
    )
    .parse::<Uri>()
    .map_err(|e| SocksProxyError::SocksConnect(e.into()))?;

    // TODO: can `Scheme::from_static()` be const fn, compare with a SOCKS5 constant?
    match proxy.uri().scheme_str() {
        Some("socks4") | Some("socks4a") => {
            let mut svc = SocksV4::new(proxy_uri, http_connector);
            let stream = Service::call(&mut svc, dst_uri)
                .await
                .map_err(|e| SocksProxyError::SocksConnect(e.into()))?;
            Ok(stream.into_inner())
        }
        Some("socks5") | Some("socks5h") => {
            let mut svc = if let Some((username, password)) = proxy.raw_auth() {
                SocksV5::new(proxy_uri, http_connector)
                    .with_auth(username.to_string(), password.to_string())
            } else {
                SocksV5::new(proxy_uri, http_connector)
            };
            let stream = Service::call(&mut svc, dst_uri)
                .await
                .map_err(|e| SocksProxyError::SocksConnect(e.into()))?;
            Ok(stream.into_inner())
        }
        _ => unreachable!(),
    }
}

impl std::fmt::Display for SocksProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SocksNoHostInUrl => f.write_str("socks proxy destination has no host"),
            Self::SocksLocalResolve(_) => f.write_str("error resolving for socks proxy"),
            Self::SocksConnect(_) => f.write_str("error connecting to socks proxy"),
        }
    }
}

impl std::error::Error for SocksProxyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SocksNoHostInUrl => None,
            Self::SocksLocalResolve(ref e) => Some(&**e),
            Self::SocksConnect(ref e) => Some(&**e),
        }
    }
}
