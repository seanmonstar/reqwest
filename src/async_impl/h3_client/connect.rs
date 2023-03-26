use crate::async_impl::h3_client::dns::resolve;
use crate::dns::DynResolver;
use crate::error::BoxError;
use bytes::Bytes;
use futures_core::future::BoxFuture;
use futures_util::{FutureExt, TryFutureExt};
use h3::client::SendRequest;
use http::Uri;
use hyper::client::connect::dns::Name;
use quinn::{ClientConfig, ConnectError, Connecting, Endpoint, TransportConfig};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};

pub type H3Connection<C, T> = (h3::client::Connection<C, Bytes>, SendRequest<T, Bytes>);

#[derive(Clone)]
pub(crate) struct H3Connector {
    resolver: DynResolver,
    connection_provider: Box<dyn H3ConnectionProvider<C = (), T = ()>>,
}

impl H3Connector {
    pub fn new<C, T>(
        resolver: DynResolver,
        connection_provider: Box<dyn H3ConnectionProvider<C = C, T = T>>,
    ) -> H3Connector {
        Self {
            resolver,
            connection_provider,
        }
    }

    pub async fn connect(&mut self, dest: Uri) -> Result<H3Connection, BoxError> {
        let host = dest.host().ok_or("destination must have a host")?;
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

        self.remote_connect(addrs, host).await
    }

    async fn remote_connect(
        &mut self,
        addrs: Vec<SocketAddr>,
        server_name: &str,
    ) -> Result<H3Connection<C, T>, BoxError> {
        let mut err = None;
        for addr in addrs {
            match self
                .connection_provider
                .poll_connect(addr, server_name)
                .await
            {
                Ok(new_conn) => {
                    return Ok(new_conn);
                }
                Err(e) => err = Some(e),
            }
        }

        match err {
            Some(e) => Err(e),
            None => Err("failed to establish connection for HTTP/3 request".into()),
        }
    }
}

pub trait H3ConnectionProvider {
    type C;
    type T;
    fn poll_connect(
        &self,
        addr: SocketAddr,
        server_name: &str,
    ) -> BoxFuture<Result<H3Connection<Self::C, Self::T>, BoxError>>;
}

pub struct QuinnH3ConnectionProvider {
    endpoint: Endpoint,
}

impl QuinnH3ConnectionProvider {
    pub fn new(
        tls: rustls::ClientConfig,
        local_addr: Option<IpAddr>,
        transport_config: TransportConfig,
    ) -> Self {
        let mut config = ClientConfig::new(Arc::new(tls));
        // FIXME: Replace this when there is a setter.
        config.transport = Arc::new(transport_config);

        let socket_addr = match local_addr {
            Some(ip) => SocketAddr::new(ip, 0),
            None => "[::]:0".parse::<SocketAddr>().unwrap(),
        };

        let mut endpoint = Endpoint::client(socket_addr).expect("unable to create QUIC endpoint");
        endpoint.set_default_client_config(config);
        QuinnH3ConnectionProvider { endpoint }
    }
}

impl H3ConnectionProvider for QuinnH3ConnectionProvider {
    type C = h3_quinn::Connection;
    type T = h3_quinn::OpenStreams;

    fn poll_connect(
        &self,
        addr: SocketAddr,
        server_name: &str,
    ) -> BoxFuture<Result<Self::C, BoxError>> {
        let connecting = self.endpoint.connect(addr, server_name);
        Box::pin(H3QuinnConnecting { connecting })
    }
}

struct H3QuinnConnecting {
    connecting: Result<Connecting, ConnectError>,
}

impl Future for H3QuinnConnecting {
    type Output = Result<H3Connection<h3_quinn::Connection, h3_quinn::OpenStreams>, BoxError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.connecting.as_ref() {
            Ok(mut connecting) => connecting
                .poll_unpin(cx)
                .map(|conn_result| match conn_result {
                    Ok(new_conn) => {
                        let quinn_conn = h3_quinn::Connection::new(new_conn);
                        return Ok(quinn_conn);
                    }
                    Err(e) => Err(Box::new(e) as BoxError),
                })
                .map_ok(|quinn_conn| h3::client::new(quinn_conn)),
            Err(e) => Poll::Ready(Err(Box::new(e) as BoxError)),
        }
    }
}
