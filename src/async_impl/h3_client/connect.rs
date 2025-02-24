use crate::async_impl::h3_client::dns::resolve;
use crate::dns::DynResolver;
use crate::error::BoxError;
use crate::tls::DynamicRustlsConfig;
use bytes::Bytes;
use h3::client::SendRequest;
use h3_quinn::{Connection, OpenStreams};
use http::Uri;
use hyper_util::client::legacy::connect::dns::Name;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

type H3Connection = (
    h3::client::Connection<Connection, Bytes>,
    SendRequest<OpenStreams, Bytes>,
);

#[derive(Clone)]
pub(crate) struct H3Connector {
    resolver: DynResolver,
    endpoint: Endpoint,
    tls: Arc<dyn DynamicRustlsConfig>,
    transport_config: Arc<TransportConfig>,
}

impl H3Connector {
    pub fn new(
        resolver: DynResolver,
        tls: Arc<dyn DynamicRustlsConfig>,
        local_addr: Option<IpAddr>,
        transport_config: TransportConfig,
    ) -> Result<H3Connector, BoxError> {
        let socket_addr = match local_addr {
            Some(ip) => SocketAddr::new(ip, 0),
            None => "[::]:0".parse::<SocketAddr>().unwrap(),
        };

        let endpoint = Endpoint::client(socket_addr)?;

        Ok(Self {
            resolver,
            endpoint,
            tls,
            transport_config: Arc::new(transport_config),
        })
    }

    pub async fn connect(&mut self, dest: Uri) -> Result<H3Connection, BoxError> {
        let host = dest
            .host()
            .ok_or("destination must have a host")?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let port = dest.port_u16().unwrap_or(443);

        let addrs = if let Some(addr) = IpAddr::from_str(host).ok() {
            // If the host is already an IP address, skip resolving.
            vec![SocketAddr::new(addr, port)]
        } else {
            let addrs = resolve(&mut self.resolver, Name::from_str(host)?).await?;
            let addrs = addrs.map(|mut addr| {
                addr.set_port(port);
                addr
            });
            addrs.collect()
        };

        let tls = self.tls.config(&dest);
        let quic_client_config = Arc::new(QuicClientConfig::try_from(tls)?);
        let mut config = ClientConfig::new(quic_client_config);
        // FIXME: Replace this when there is a setter.
        config.transport_config(self.transport_config.clone());

        self.remote_connect(config, addrs, host).await
    }

    async fn remote_connect(
        &mut self,
        config: ClientConfig,
        addrs: Vec<SocketAddr>,
        server_name: &str,
    ) -> Result<H3Connection, BoxError> {
        let mut err = None;

        for addr in addrs {
            match self
                .endpoint
                .connect_with(config.clone(), addr, server_name)?
                .await
            {
                Ok(new_conn) => {
                    let quinn_conn = Connection::new(new_conn);
                    return Ok(h3::client::new(quinn_conn).await?);
                }
                Err(e) => err = Some(e),
            }
        }

        match err {
            Some(e) => Err(Box::new(e) as BoxError),
            None => Err("failed to establish connection for HTTP/3 request".into()),
        }
    }
}
