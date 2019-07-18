use std::fmt;
#[cfg(feature = "rustls-tls")]
use rustls::{TLSError, ServerCertVerifier, RootCertStore, ServerCertVerified};
#[cfg(feature = "rustls-tls")]
use tokio_rustls::webpki::DNSNameRef;

/// Represent a server X509 certificate.
#[derive(Clone)]
pub struct Certificate {
    #[cfg(feature = "default-tls")]
    native: ::native_tls::Certificate,
    #[cfg(feature = "rustls-tls")]
    original: Cert,
}

#[cfg(feature = "rustls-tls")]
#[derive(Clone)]
enum Cert {
    Der(Vec<u8>),
    Pem(Vec<u8>)
}

/// Represent a private key and X509 cert as a client certificate.
pub struct Identity {
    inner: ClientCert,
}

enum ClientCert {
    #[cfg(feature = "default-tls")]
    Pkcs12(::native_tls::Identity),
    #[cfg(feature = "rustls-tls")]
    Pem {
        key: ::rustls::PrivateKey,
        certs: Vec<::rustls::Certificate>,
    }
}

impl Certificate {
    /// Create a `Certificate` from a binary DER encoded certificate
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn cert() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my_cert.der")?
    ///     .read_to_end(&mut buf)?;
    /// let cert = reqwest::Certificate::from_der(&buf)?;
    /// # drop(cert);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_der(der: &[u8]) -> ::Result<Certificate> {
        Ok(Certificate {
            #[cfg(feature = "default-tls")]
            native: try_!(::native_tls::Certificate::from_der(der)),
            #[cfg(feature = "rustls-tls")]
            original: Cert::Der(der.to_owned()),
        })
    }


    /// Create a `Certificate` from a PEM encoded certificate
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn cert() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my_cert.pem")?
    ///     .read_to_end(&mut buf)?;
    /// let cert = reqwest::Certificate::from_pem(&buf)?;
    /// # drop(cert);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_pem(pem: &[u8]) -> ::Result<Certificate> {
        Ok(Certificate {
            #[cfg(feature = "default-tls")]
            native: try_!(::native_tls::Certificate::from_pem(pem)),
            #[cfg(feature = "rustls-tls")]
            original: Cert::Pem(pem.to_owned())
        })
    }

    #[cfg(feature = "default-tls")]
    pub(crate) fn add_to_native_tls(
        self,
        tls: &mut ::native_tls::TlsConnectorBuilder,
    ) {
        tls.add_root_certificate(self.native);
    }

    #[cfg(feature = "rustls-tls")]
    pub(crate) fn add_to_rustls(
        self,
        tls: &mut ::rustls::ClientConfig,
    ) -> ::Result<()> {
        use std::io::Cursor;
        use rustls::internal::pemfile;

        match self.original {
            Cert::Der(buf) => try_!(tls.root_store.add(&::rustls::Certificate(buf))
                .map_err(TLSError::WebPKIError)),
            Cert::Pem(buf) => {
                let mut pem = Cursor::new(buf);
                let certs = try_!(pemfile::certs(&mut pem)
                    .map_err(|_| TLSError::General(String::from("No valid certificate was found"))));
                for c in certs {
                    try_!(tls.root_store.add(&c)
                        .map_err(TLSError::WebPKIError));
                }
            }
        }
        Ok(())
    }
}

impl Identity {
    /// Parses a DER-formatted PKCS #12 archive, using the specified password to decrypt the key.
    ///
    /// The archive should contain a leaf certificate and its private key, as well any intermediate
    /// certificates that allow clients to build a chain to a trusted root.
    /// The chain certificates should be in order from the leaf certificate towards the root.
    ///
    /// PKCS #12 archives typically have the file extension `.p12` or `.pfx`, and can be created
    /// with the OpenSSL `pkcs12` tool:
    ///
    /// ```bash
    /// openssl pkcs12 -export -out identity.pfx -inkey key.pem -in cert.pem -certfile chain_certs.pem
    /// ```
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn pkcs12() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my-ident.pfx")?
    ///     .read_to_end(&mut buf)?;
    /// let pkcs12 = reqwest::Identity::from_pkcs12_der(&buf, "my-privkey-password")?;
    /// # drop(pkcs12);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "default-tls")]
    pub fn from_pkcs12_der(der: &[u8], password: &str) -> ::Result<Identity> {
        Ok(Identity {
            inner: ClientCert::Pkcs12(
                try_!(::native_tls::Identity::from_pkcs12(der, password))
            ),
        })
    }

    /// Parses PEM encoded private key and certificate.
    ///
    /// The input should contain a PEM encoded private key
    /// and at least one PEM encoded certificate.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn pem() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my-ident.pem")?
    ///     .read_to_end(&mut buf)?;
    /// let id = reqwest::Identity::from_pem(&buf)?;
    /// # drop(id);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "rustls-tls")]
    pub fn from_pem(buf: &[u8]) -> ::Result<Identity> {
        use std::io::Cursor;
        use rustls::internal::pemfile;

        let (key, certs) = {
            let mut pem = Cursor::new(buf);
            let certs = try_!(pemfile::certs(&mut pem)
                .map_err(|_| TLSError::General(String::from("No valid certificate was found"))));
            pem.set_position(0);
            let mut sk = try_!(pemfile::pkcs8_private_keys(&mut pem)
                .and_then(|pkcs8_keys| {
                    if pkcs8_keys.is_empty() {
                        Err(())
                    } else {
                        Ok(pkcs8_keys)
                    }
                })
                .or_else(|_| {
                    pem.set_position(0);
                    pemfile::rsa_private_keys(&mut pem)
                })
                .map_err(|_| TLSError::General(String::from("No valid private key was found"))));
            if let (Some(sk), false) = (sk.pop(), certs.is_empty()) {
                (sk, certs)
            } else {
                return Err(::error::from(TLSError::General(String::from("private key or certificate not found"))));
            }
        };

        Ok(Identity {
            inner: ClientCert::Pem {
                key,
                certs,
            },
        })
    }

    #[cfg(feature = "default-tls")]
    pub(crate) fn add_to_native_tls(
        self,
        tls: &mut ::native_tls::TlsConnectorBuilder,
    ) -> ::Result<()> {
        match self.inner {
            ClientCert::Pkcs12(id) => {
                tls.identity(id);
                Ok(())
            },
            #[cfg(feature = "rustls-tls")]
            ClientCert::Pem { .. } => Err(::error::from(::error::Kind::TlsIncompatible))
        }
    }

    #[cfg(feature = "rustls-tls")]
    pub(crate) fn add_to_rustls(
        self,
        tls: &mut ::rustls::ClientConfig,
    ) -> ::Result<()> {
        match self.inner {
            ClientCert::Pem { key, certs } => {
                tls.set_single_client_cert(certs, key);
                Ok(())
            },
            #[cfg(feature = "default-tls")]
            ClientCert::Pkcs12(..) => return Err(::error::from(::error::Kind::TlsIncompatible))
        }
    }
}

impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Certificate")
            .finish()
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Identity")
            .finish()
    }
}

pub(crate) enum TlsBackend {
    #[cfg(feature = "default-tls")]
    Default,
    #[cfg(feature = "rustls-tls")]
    Rustls
}

impl Default for TlsBackend {
    fn default() -> TlsBackend {
        #[cfg(feature = "default-tls")]
        { TlsBackend::Default }

        #[cfg(all(feature = "rustls-tls", not(feature = "default-tls")))]
        { TlsBackend::Rustls }
    }
}

#[cfg(feature = "rustls-tls")]
pub(crate) struct NoVerifier;

#[cfg(feature = "rustls-tls")]
impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _roots: &RootCertStore,
        _presented_certs: &[rustls::Certificate],
        _dns_name: DNSNameRef,
        _ocsp_response: &[u8]
    ) -> Result<ServerCertVerified, TLSError> {
        Ok(ServerCertVerified::assertion())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "default-tls")]
    #[test]
    fn certificate_from_der_invalid() {
        Certificate::from_der(b"not der").unwrap_err();
    }

    #[cfg(feature = "default-tls")]
    #[test]
    fn certificate_from_pem_invalid() {
        Certificate::from_pem(b"not pem").unwrap_err();
    }

    #[cfg(feature = "default-tls")]
    #[test]
    fn identity_from_pkcs12_der_invalid() {
        Identity::from_pkcs12_der(b"not der", "nope").unwrap_err();
    }

    #[cfg(feature = "rustls-tls")]
    #[test]
    fn identity_from_pem_invalid() {
        Identity::from_pem(b"not pem").unwrap_err();
    }

    #[cfg(feature = "rustls-tls")]
    #[test]
    fn identity_from_pem_pkcs1_key() {
        let pem = b"-----BEGIN CERTIFICATE-----\n\
            -----END CERTIFICATE-----\n\
            -----BEGIN RSA PRIVATE KEY-----\n\
            -----END RSA PRIVATE KEY-----\n";

        Identity::from_pem(pem).unwrap();
    }
}
