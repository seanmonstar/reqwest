use crate::error::BoxError;
use bytes::Bytes;
use h3::client::SendRequest;
use iroh::{Endpoint, EndpointAddr};
use iroh_tickets::endpoint::EndpointTicket;
use std::str::FromStr;

use super::connection::{Connection, OpenStreams};

type Iroh3Connection = (
    h3::client::Connection<Connection, Bytes>,
    SendRequest<OpenStreams, Bytes>,
);

/// H3 Client Config
#[derive(Clone)]
pub(crate) struct Iroh3ClientConfig {
    /// Set the maximum HTTP/3 header size this client is willing to accept.
    ///
    /// See [header size constraints] section of the specification for details.
    ///
    /// [header size constraints]: https://www.rfc-editor.org/rfc/rfc9114.html#name-header-size-constraints
    ///
    /// Please see docs in [`Builder`] in [`h3`].
    ///
    /// [`Builder`]: https://docs.rs/h3/latest/h3/client/struct.Builder.html#method.max_field_section_size
    pub(crate) max_field_section_size: Option<u64>,

    /// Enable whether to send HTTP/3 protocol grease on the connections.
    ///
    /// Just like in HTTP/2, HTTP/3 also uses the concept of "grease"
    ///
    /// to prevent potential interoperability issues in the future.
    /// In HTTP/3, the concept of grease is used to ensure that the protocol can evolve
    /// and accommodate future changes without breaking existing implementations.
    ///
    /// Please see docs in [`Builder`] in [`h3`].
    ///
    /// [`Builder`]: https://docs.rs/h3/latest/h3/client/struct.Builder.html#method.send_grease
    pub(crate) send_grease: Option<bool>,
}

impl Default for Iroh3ClientConfig {
    fn default() -> Self {
        Self {
            max_field_section_size: None,
            send_grease: None,
        }
    }
}

static ENDPOINT: tokio::sync::OnceCell<Endpoint> = tokio::sync::OnceCell::const_new();

#[derive(Clone)]
pub(crate) struct Iroh3Connector {
    client_config: Iroh3ClientConfig,
}

impl Iroh3Connector {
    pub fn new(
        transport_config: iroh::endpoint::TransportConfig,
        client_config: Iroh3ClientConfig,
    ) -> Result<Iroh3Connector, BoxError> {
        tokio::task::spawn(async move {
            ENDPOINT.get_or_try_init(|| async {
                let endpoint = Endpoint::builder()
                    .transport_config(transport_config)
                    .bind()
                    .await?;
                Ok::<Endpoint, BoxError>(endpoint)
            });
        });

        Ok(Self { client_config })
    }

    pub async fn connect(&mut self, dest: &str) -> Result<Iroh3Connection, BoxError> {
        match EndpointTicket::from_str(dest) {
            Ok(ticket) => self.remote_connect(ticket.into()).await,
            Err(e) => Err(e.into()),
        }
    }

    async fn remote_connect(&mut self, addr: EndpointAddr) -> Result<Iroh3Connection, BoxError> {
        match ENDPOINT.get() {
            Some(endpoint) => match endpoint.connect(addr, b"iroh+h3").await {
                Ok(conn) => {
                    let quinn_conn = Connection::new(conn);
                    let mut h3_client_builder = h3::client::builder();
                    if let Some(max_field_section_size) = self.client_config.max_field_section_size
                    {
                        h3_client_builder.max_field_section_size(max_field_section_size);
                    }
                    if let Some(send_grease) = self.client_config.send_grease {
                        h3_client_builder.send_grease(send_grease);
                    }
                    return Ok(h3_client_builder.build(quinn_conn).await?);
                }
                Err(e) => Err(e.into()),
            },
            None => Err("endpoint not initialized".into()),
        }
    }
}
