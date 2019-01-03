use std::fmt;
#[cfg(feature = "rustls-tls")]
use rustls::{TLSError, ServerCertVerifier, RootCertStore, ServerCertVerified};
#[cfg(feature = "rustls-tls")]
use tokio_rustls::webpki::DNSNameRef;

/// Represent an X509 certificate.
pub struct Certificate {
    pub(crate) inner: inner::Certificate
}

/// Represent a private key and X509 cert as a client certificate.
pub struct Identity {
    pub(crate) inner: inner::Identity
}

pub(crate) mod inner {
    pub(crate) enum Certificate {
        Der(Vec<u8>),
        Pem(Vec<u8>)
    }

    pub(crate) enum Identity {
        #[cfg(feature = "default-tls")]
        Pkcs12(Vec<u8>, String),
        #[cfg(feature = "rustls-tls")]
        Pem(Vec<u8>),
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
    ///
    /// # Errors
    ///
    /// It never returns error.
    pub fn from_der(der: &[u8]) -> ::Result<Certificate> {
        Ok(Certificate {
            inner: inner::Certificate::Der(der.to_owned())
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
    ///
    /// # Errors
    ///
    /// It never returns error.
    pub fn from_pem(der: &[u8]) -> ::Result<Certificate> {
        Ok(Certificate {
            inner: inner::Certificate::Pem(der.to_owned())
        })
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
    ///
    /// # Errors
    ///
    /// It never returns error.
    #[cfg(feature = "default-tls")]
    pub fn from_pkcs12_der(der: &[u8], password: &str) -> ::Result<Identity> {
        Ok(Identity {
            inner: inner::Identity::Pkcs12(der.to_owned(), password.to_owned())
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
    ///
    /// # Errors
    ///
    /// It never returns error.
    #[cfg(feature = "rustls-tls")]
    pub fn from_pem(pem: &[u8]) -> ::Result<Identity> {
        Ok(Identity {
            inner: inner::Identity::Pem(pem.to_owned())
        })
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
