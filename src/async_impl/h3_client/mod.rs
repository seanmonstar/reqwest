use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::SystemTime;
use rustls::client::ServerCertVerified;
use rustls::{Error, ServerName};
use bytes::Bytes;
use h3::client::SendRequest;
use http::{Request, Response, Uri};
use crate::error::BoxError;
use hyper::Body;
use bytes::Buf;
use futures_util::future;

static ALPN: &[u8] = b"h3";

// hyper Client
#[derive(Clone)]
pub struct H3Client {
    connector: H3Connector,
}

impl H3Client {
    #[cfg(feature = "__rustls")]
    pub fn new() -> Self {
        let tls_config_builder = rustls::ClientConfig::builder()
            .with_safe_default_cipher_suites()
            .with_safe_default_kx_groups()
            .with_protocol_versions(&[&rustls::version::TLS13]).unwrap();
        let mut tls_config = tls_config_builder
            .with_custom_certificate_verifier(Arc::new(YesVerifier))
            .with_no_client_auth();

        tls_config.enable_early_data = true;
        tls_config.alpn_protocols = vec![ALPN.into()];

        Self {
            connector: H3Connector {
                config: quinn::ClientConfig::new(Arc::new(tls_config)),
            },
        }
    }

    pub(super) fn request(&self, req: Request<()>) -> H3ResponseFuture {
        // Connect via connector
        //H3ResponseFuture{inner: Box::pin(self.clone().connect_request(req))}
        let mut connector = self.connector.clone();
        H3ResponseFuture{inner: Box::pin(async move {
            eprintln!("Trying http3 ...");
            let mut send_request = connector.connect_to(req.uri().clone()).await.unwrap();
            let mut stream = send_request.send_request(req).await.unwrap();
            stream.finish().await.unwrap();

            eprintln!("Receiving response ...");
            let resp = stream.recv_response().await.unwrap();
            eprintln!("Response h3 {:?}", resp);

            while let Some(chunk) = stream.recv_data().await.unwrap() {
                eprintln!("Chunk: {:?}", chunk.chunk());
            }

            Ok(resp.map(|_| {
                Body::empty()
            }))
        })}
    }
}

struct YesVerifier;

impl rustls::client::ServerCertVerifier for YesVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }
}

// hyper HttpConnector
#[derive(Clone)]
pub struct H3Connector {
    // TODO: is cloning this config expensive?
    config: quinn::ClientConfig,
}

impl H3Connector {
    async fn connect_to(&mut self, dest: Uri) -> Result<SendRequest<h3_quinn::OpenStreams, Bytes>, BoxError> {
        let auth = dest
            .authority()
            .ok_or("destination must have a host")?
            .clone();
        let port = auth.port_u16().unwrap_or(443);
        let addr = tokio::net::lookup_host((auth.host(), port))
            .await?
            .next()
            .ok_or("dns found no addresses")?;
        eprintln!("URI {}", dest);
        let mut client_endpoint = h3_quinn::quinn::Endpoint::client("[::]:0".parse().unwrap())?;
        client_endpoint.set_default_client_config(self.config.clone());
        let quinn_conn = h3_quinn::Connection::new(client_endpoint.connect(addr, auth.host())?.await?);
        let (mut driver, send_request) = h3::client::new(quinn_conn).await?;
        tokio::spawn(async move {
            future::poll_fn(|cx| driver.poll_close(cx)).await.unwrap();
        });
        Ok(send_request)
    }
}


pub struct H3ResponseFuture {
    inner: Pin<Box<dyn Future<Output = Result<Response<Body>, crate::Error>> + Send>>,
}


impl Future for H3ResponseFuture {
    type Output = Result<Response<Body>, crate::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}
