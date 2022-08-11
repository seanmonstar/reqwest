use crate::async_impl::h3_client::dns::Resolver;
use crate::error::BoxError;
use bytes::Bytes;
use futures_util::future;
use h3::client::SendRequest;
use h3_quinn::{Connection, OpenStreams};
use http::Uri;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct H3Connector {
    resolver: Resolver,
    endpoint: quinn::Endpoint,
}

impl H3Connector {
    pub fn new(
        resolver: Resolver,
        tls: rustls::ClientConfig,
        local_addr: Option<IpAddr>,
    ) -> H3Connector {
        let config = quinn::ClientConfig::new(Arc::new(tls));

        let socket_addr = match local_addr {
            Some(ip) => SocketAddr::new(ip, 0),
            None => "[::]:0".parse::<SocketAddr>().unwrap(),
        };

        let mut endpoint =
            quinn::Endpoint::client(socket_addr).expect("unable to create QUIC endpoint");
        endpoint.set_default_client_config(config);

        Self { resolver, endpoint }
    }

    pub async fn connect(
        &mut self,
        dest: Uri,
    ) -> Result<SendRequest<OpenStreams, Bytes>, BoxError> {
        let host = dest.host().ok_or("destination must have a host")?;
        let port = dest.port_u16().unwrap_or(443);

        let addrs = if let Some(addr) = IpAddr::from_str(host).ok() {
            // If the host is already an IP address, skip resolving.
            vec![SocketAddr::new(addr, port)]
        } else {
            let addrs = self.resolver.resolve(host).await.into_iter();
            let addrs = addrs.map(|mut addr| {
                addr.set_port(port);
                addr
            });
            addrs.collect()
        };

        self.remote_connect(addrs, host).await
    }

    async fn remote_connect(
        &mut self,
        addrs: Vec<SocketAddr>,
        server_name: &str,
    ) -> Result<SendRequest<OpenStreams, Bytes>, BoxError> {
        let mut err = None;
        for addr in addrs {
            match self.endpoint.connect(addr, server_name)?.await {
                Ok(new_conn) => {
                    let quinn_conn = Connection::new(new_conn);
                    let (mut driver, tx) = h3::client::new(quinn_conn).await?;
                    tokio::spawn(async move {
                        future::poll_fn(|cx| driver.poll_close(cx)).await.unwrap();
                    });
                    return Ok(tx);
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
