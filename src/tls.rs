//! TLS configuration and types
//!
//! A `Client` will use transport layer security (TLS) by default to connect to
//! HTTPS destinations.
//!
//! # Backends
//!
//! reqwest supports several TLS backends, enabled with Cargo features.
//!
//! ## default-tls
//!
//! reqwest will pick a TLS backend by default. This is true when the
//! `default-tls` feature is enabled.
//!
//! While it currently uses `native-tls`, the feature set is designed to only
//! enable configuration that is shared among available backends. This allows
//! reqwest to change the default to `rustls` (or another) at some point in the
//! future.
//!
//! <div class="warning">This feature is enabled by default, and takes
//! precedence if any other crate enables it. This is true even if you declare
//! `features = []`. You must set `default-features = false` instead.</div>
//!
//! Since Cargo features are additive, other crates in your dependency tree can
//! cause the default backend to be enabled. If you wish to ensure your
//! `Client` uses a specific backend, call the appropriate builder methods
//! (such as [`use_rustls_tls()`][]).
//!
//! [`use_rustls_tls()`]: crate::ClientBuilder::use_rustls_tls()
//!
//! ## native-tls
//!
//! This backend uses the [native-tls][] crate. That will try to use the system
//! TLS on Windows and Mac, and OpenSSL on Linux targets.
//!
//! Enabling the feature explicitly allows for `native-tls`-specific
//! configuration options.
//!
//! [native-tls]: https://crates.io/crates/native-tls
//!
//! ## rustls-tls
//!
//! This backend uses the [rustls][] crate, a TLS library written in Rust.
//!
//! [rustls]: https://crates.io/crates/rustls

#[cfg(feature = "__rustls")]
use rustls::{
    client::HandshakeSignatureValid, client::ServerCertVerified, client::ServerCertVerifier,
    DigitallySignedStruct, Error as TLSError, ServerName,
};
use std::{
    fmt,
    io::{BufRead, BufReader},
};

/// Represents a server X509 certificate.
#[derive(Clone)]
pub struct Certificate {
    #[cfg(feature = "native-tls-crate")]
    native: native_tls_crate::Certificate,
    #[cfg(feature = "__rustls")]
    original: Cert,
}

#[cfg(feature = "__rustls")]
#[derive(Clone)]
enum Cert {
    Der(Vec<u8>),
    Pem(Vec<u8>),
}

/// Represents a private key and X509 cert as a client certificate.
#[derive(Clone)]
pub struct Identity {
    #[cfg_attr(not(any(feature = "native-tls", feature = "__rustls")), allow(unused))]
    inner: ClientCert,
}

#[derive(Clone)]
enum ClientCert {
    #[cfg(feature = "native-tls")]
    Pkcs12(native_tls_crate::Identity),
    #[cfg(feature = "native-tls")]
    Pkcs8(native_tls_crate::Identity),
    #[cfg(feature = "__rustls")]
    Pem {
        key: rustls::PrivateKey,
        certs: Vec<rustls::Certificate>,
    },
}

impl Certificate {
    /// Create a `Certificate` from a binary DER encoded certificate
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn cert() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my_cert.der")?
    ///     .read_to_end(&mut buf)?;
    /// let cert = reqwest::Certificate::from_der(&buf)?;
    /// # drop(cert);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_der(der: &[u8]) -> crate::Result<Certificate> {
        Ok(Certificate {
            #[cfg(feature = "native-tls-crate")]
            native: native_tls_crate::Certificate::from_der(der).map_err(crate::error::builder)?,
            #[cfg(feature = "__rustls")]
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
    /// # fn cert() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my_cert.pem")?
    ///     .read_to_end(&mut buf)?;
    /// let cert = reqwest::Certificate::from_pem(&buf)?;
    /// # drop(cert);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_pem(pem: &[u8]) -> crate::Result<Certificate> {
        Ok(Certificate {
            #[cfg(feature = "native-tls-crate")]
            native: native_tls_crate::Certificate::from_pem(pem).map_err(crate::error::builder)?,
            #[cfg(feature = "__rustls")]
            original: Cert::Pem(pem.to_owned()),
        })
    }

    /// Create a collection of `Certificate`s from a PEM encoded certificate bundle.
    /// Example byte sources may be `.crt`, `.cer` or `.pem` files.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn cert() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("ca-bundle.crt")?
    ///     .read_to_end(&mut buf)?;
    /// let certs = reqwest::Certificate::from_pem_bundle(&buf)?;
    /// # drop(certs);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_pem_bundle(pem_bundle: &[u8]) -> crate::Result<Vec<Certificate>> {
        let mut reader = BufReader::new(pem_bundle);

        Self::read_pem_certs(&mut reader)?
            .iter()
            .map(|cert_vec| Certificate::from_der(&cert_vec))
            .collect::<crate::Result<Vec<Certificate>>>()
    }

    #[cfg(feature = "native-tls-crate")]
    pub(crate) fn add_to_native_tls(self, tls: &mut native_tls_crate::TlsConnectorBuilder) {
        tls.add_root_certificate(self.native);
    }

    #[cfg(feature = "__rustls")]
    pub(crate) fn add_to_rustls(
        self,
        root_cert_store: &mut rustls::RootCertStore,
    ) -> crate::Result<()> {
        use std::io::Cursor;

        match self.original {
            Cert::Der(buf) => root_cert_store
                .add(&rustls::Certificate(buf))
                .map_err(crate::error::builder)?,
            Cert::Pem(buf) => {
                let mut reader = Cursor::new(buf);
                let certs = Self::read_pem_certs(&mut reader)?;
                for c in certs {
                    root_cert_store
                        .add(&rustls::Certificate(c))
                        .map_err(crate::error::builder)?;
                }
            }
        }
        Ok(())
    }

    fn read_pem_certs(reader: &mut impl BufRead) -> crate::Result<Vec<Vec<u8>>> {
        rustls_pemfile::certs(reader)
            .map_err(|_| crate::error::builder("invalid certificate encoding"))
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
    /// # fn pkcs12() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my-ident.pfx")?
    ///     .read_to_end(&mut buf)?;
    /// let pkcs12 = reqwest::Identity::from_pkcs12_der(&buf, "my-privkey-password")?;
    /// # drop(pkcs12);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Optional
    ///
    /// This requires the `native-tls` Cargo feature enabled.
    #[cfg(feature = "native-tls")]
    pub fn from_pkcs12_der(der: &[u8], password: &str) -> crate::Result<Identity> {
        Ok(Identity {
            inner: ClientCert::Pkcs12(
                native_tls_crate::Identity::from_pkcs12(der, password)
                    .map_err(crate::error::builder)?,
            ),
        })
    }

    /// Parses a chain of PEM encoded X509 certificates, with the leaf certificate first.
    /// `key` is a PEM encoded PKCS #8 formatted private key for the leaf certificate.
    ///
    /// The certificate chain should contain any intermediate cerficates that should be sent to
    /// clients to allow them to build a chain to a trusted root.
    ///
    /// A certificate chain here means a series of PEM encoded certificates concatenated together.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs;
    /// # fn pkcs8() -> Result<(), Box<dyn std::error::Error>> {
    /// let cert = fs::read("client.pem")?;
    /// let key = fs::read("key.pem")?;
    /// let pkcs8 = reqwest::Identity::from_pkcs8_pem(&cert, &key)?;
    /// # drop(pkcs8);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Optional
    ///
    /// This requires the `native-tls` Cargo feature enabled.
    #[cfg(feature = "native-tls")]
    pub fn from_pkcs8_pem(pem: &[u8], key: &[u8]) -> crate::Result<Identity> {
        Ok(Identity {
            inner: ClientCert::Pkcs8(
                native_tls_crate::Identity::from_pkcs8(pem, key).map_err(crate::error::builder)?,
            ),
        })
    }

    /// Parses PEM encoded private key and certificate.
    ///
    /// The input should contain a PEM encoded private key
    /// and at least one PEM encoded certificate.
    ///
    /// Note: The private key must be in RSA, SEC1 Elliptic Curve or PKCS#8 format.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn pem() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my-ident.pem")?
    ///     .read_to_end(&mut buf)?;
    /// let id = reqwest::Identity::from_pem(&buf)?;
    /// # drop(id);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Optional
    ///
    /// This requires the `rustls-tls(-...)` Cargo feature enabled.
    #[cfg(feature = "__rustls")]
    pub fn from_pem(buf: &[u8]) -> crate::Result<Identity> {
        use std::io::Cursor;

        let (key, certs) = {
            let mut pem = Cursor::new(buf);
            let mut sk = Vec::<rustls::PrivateKey>::new();
            let mut certs = Vec::<rustls::Certificate>::new();

            for item in std::iter::from_fn(|| rustls_pemfile::read_one(&mut pem).transpose()) {
                match item.map_err(|_| {
                    crate::error::builder(TLSError::General(String::from(
                        "Invalid identity PEM file",
                    )))
                })? {
                    rustls_pemfile::Item::X509Certificate(cert) => {
                        certs.push(rustls::Certificate(cert))
                    }
                    rustls_pemfile::Item::PKCS8Key(key) => sk.push(rustls::PrivateKey(key)),
                    rustls_pemfile::Item::RSAKey(key) => sk.push(rustls::PrivateKey(key)),
                    rustls_pemfile::Item::ECKey(key) => sk.push(rustls::PrivateKey(key)),
                    _ => {
                        return Err(crate::error::builder(TLSError::General(String::from(
                            "No valid certificate was found",
                        ))))
                    }
                }
            }

            if let (Some(sk), false) = (sk.pop(), certs.is_empty()) {
                (sk, certs)
            } else {
                return Err(crate::error::builder(TLSError::General(String::from(
                    "private key or certificate not found",
                ))));
            }
        };

        Ok(Identity {
            inner: ClientCert::Pem { key, certs },
        })
    }

    #[cfg(feature = "native-tls")]
    pub(crate) fn add_to_native_tls(
        self,
        tls: &mut native_tls_crate::TlsConnectorBuilder,
    ) -> crate::Result<()> {
        match self.inner {
            ClientCert::Pkcs12(id) | ClientCert::Pkcs8(id) => {
                tls.identity(id);
                Ok(())
            }
            #[cfg(feature = "__rustls")]
            ClientCert::Pem { .. } => Err(crate::error::builder("incompatible TLS identity type")),
        }
    }

    #[cfg(feature = "__rustls")]
    pub(crate) fn add_to_rustls(
        self,
        config_builder: rustls::ConfigBuilder<
            rustls::ClientConfig,
            rustls::client::WantsTransparencyPolicyOrClientCert,
        >,
    ) -> crate::Result<rustls::ClientConfig> {
        match self.inner {
            ClientCert::Pem { key, certs } => config_builder
                .with_client_auth_cert(certs, key)
                .map_err(crate::error::builder),
            #[cfg(feature = "native-tls")]
            ClientCert::Pkcs12(..) | ClientCert::Pkcs8(..) => {
                Err(crate::error::builder("incompatible TLS identity type"))
            }
        }
    }
}

impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Certificate").finish()
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Identity").finish()
    }
}

/// A TLS protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(InnerVersion);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
enum InnerVersion {
    Tls1_0,
    Tls1_1,
    Tls1_2,
    Tls1_3,
}

// These could perhaps be From/TryFrom implementations, but those would be
// part of the public API so let's be careful
impl Version {
    /// Version 1.0 of the TLS protocol.
    pub const TLS_1_0: Version = Version(InnerVersion::Tls1_0);
    /// Version 1.1 of the TLS protocol.
    pub const TLS_1_1: Version = Version(InnerVersion::Tls1_1);
    /// Version 1.2 of the TLS protocol.
    pub const TLS_1_2: Version = Version(InnerVersion::Tls1_2);
    /// Version 1.3 of the TLS protocol.
    pub const TLS_1_3: Version = Version(InnerVersion::Tls1_3);

    #[cfg(feature = "default-tls")]
    pub(crate) fn to_native_tls(self) -> Option<native_tls_crate::Protocol> {
        match self.0 {
            InnerVersion::Tls1_0 => Some(native_tls_crate::Protocol::Tlsv10),
            InnerVersion::Tls1_1 => Some(native_tls_crate::Protocol::Tlsv11),
            InnerVersion::Tls1_2 => Some(native_tls_crate::Protocol::Tlsv12),
            InnerVersion::Tls1_3 => None,
        }
    }

    #[cfg(feature = "__rustls")]
    pub(crate) fn from_rustls(version: rustls::ProtocolVersion) -> Option<Self> {
        match version {
            rustls::ProtocolVersion::SSLv2 => None,
            rustls::ProtocolVersion::SSLv3 => None,
            rustls::ProtocolVersion::TLSv1_0 => Some(Self(InnerVersion::Tls1_0)),
            rustls::ProtocolVersion::TLSv1_1 => Some(Self(InnerVersion::Tls1_1)),
            rustls::ProtocolVersion::TLSv1_2 => Some(Self(InnerVersion::Tls1_2)),
            rustls::ProtocolVersion::TLSv1_3 => Some(Self(InnerVersion::Tls1_3)),
            _ => None,
        }
    }
}

pub(crate) enum TlsBackend {
    // This is the default and HTTP/3 feature does not use it so suppress it.
    #[allow(dead_code)]
    #[cfg(feature = "default-tls")]
    Default,
    #[cfg(feature = "native-tls")]
    BuiltNativeTls(native_tls_crate::TlsConnector),
    #[cfg(feature = "__rustls")]
    Rustls,
    #[cfg(feature = "__rustls")]
    BuiltRustls(rustls::ClientConfig),
    #[cfg(any(feature = "native-tls", feature = "__rustls",))]
    UnknownPreconfigured,
}

impl fmt::Debug for TlsBackend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            #[cfg(feature = "default-tls")]
            TlsBackend::Default => write!(f, "Default"),
            #[cfg(feature = "native-tls")]
            TlsBackend::BuiltNativeTls(_) => write!(f, "BuiltNativeTls"),
            #[cfg(feature = "__rustls")]
            TlsBackend::Rustls => write!(f, "Rustls"),
            #[cfg(feature = "__rustls")]
            TlsBackend::BuiltRustls(_) => write!(f, "BuiltRustls"),
            #[cfg(any(feature = "native-tls", feature = "__rustls",))]
            TlsBackend::UnknownPreconfigured => write!(f, "UnknownPreconfigured"),
        }
    }
}

impl Default for TlsBackend {
    fn default() -> TlsBackend {
        #[cfg(all(feature = "default-tls", not(feature = "http3")))]
        {
            TlsBackend::Default
        }

        #[cfg(any(
            all(feature = "__rustls", not(feature = "default-tls")),
            feature = "http3"
        ))]
        {
            TlsBackend::Rustls
        }
    }
}

#[cfg(feature = "__rustls")]
pub(crate) struct NoVerifier;

#[cfg(feature = "__rustls")]
impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<ServerCertVerified, TLSError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::Certificate,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::Certificate,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TLSError> {
        Ok(HandshakeSignatureValid::assertion())
    }
}

/// Hyper extension carrying extra TLS layer information.
/// Made available to clients on responses when `tls_info` is set.
#[derive(Clone)]
pub struct TlsInfo {
    pub(crate) peer_certificate: Option<Vec<u8>>,
}

impl TlsInfo {
    /// Get the DER encoded leaf certificate of the peer.
    pub fn peer_certificate(&self) -> Option<&[u8]> {
        self.peer_certificate.as_ref().map(|der| &der[..])
    }
}

impl std::fmt::Debug for TlsInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("TlsInfo").finish()
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

    #[cfg(feature = "native-tls")]
    #[test]
    fn identity_from_pkcs12_der_invalid() {
        Identity::from_pkcs12_der(b"not der", "nope").unwrap_err();
    }

    #[cfg(feature = "native-tls")]
    #[test]
    fn identity_from_pkcs8_pem_invalid() {
        Identity::from_pkcs8_pem(b"not pem", b"not key").unwrap_err();
    }

    #[cfg(feature = "__rustls")]
    #[test]
    fn identity_from_pem_invalid() {
        Identity::from_pem(b"not pem").unwrap_err();
    }

    #[cfg(feature = "__rustls")]
    #[test]
    fn identity_from_pem_pkcs1_key() {
        let pem = b"-----BEGIN CERTIFICATE-----\n\
            -----END CERTIFICATE-----\n\
            -----BEGIN RSA PRIVATE KEY-----\n\
            -----END RSA PRIVATE KEY-----\n";

        Identity::from_pem(pem).unwrap();
    }

    #[test]
    fn certificates_from_pem_bundle() {
        const PEM_BUNDLE: &[u8] = b"
            -----BEGIN CERTIFICATE-----
            MIIBtjCCAVugAwIBAgITBmyf1XSXNmY/Owua2eiedgPySjAKBggqhkjOPQQDAjA5
            MQswCQYDVQQGEwJVUzEPMA0GA1UEChMGQW1hem9uMRkwFwYDVQQDExBBbWF6b24g
            Um9vdCBDQSAzMB4XDTE1MDUyNjAwMDAwMFoXDTQwMDUyNjAwMDAwMFowOTELMAkG
            A1UEBhMCVVMxDzANBgNVBAoTBkFtYXpvbjEZMBcGA1UEAxMQQW1hem9uIFJvb3Qg
            Q0EgMzBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABCmXp8ZBf8ANm+gBG1bG8lKl
            ui2yEujSLtf6ycXYqm0fc4E7O5hrOXwzpcVOho6AF2hiRVd9RFgdszflZwjrZt6j
            QjBAMA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgGGMB0GA1UdDgQWBBSr
            ttvXBp43rDCGB5Fwx5zEGbF4wDAKBggqhkjOPQQDAgNJADBGAiEA4IWSoxe3jfkr
            BqWTrBqYaGFy+uGh0PsceGCmQ5nFuMQCIQCcAu/xlJyzlvnrxir4tiz+OpAUFteM
            YyRIHN8wfdVoOw==
            -----END CERTIFICATE-----

            -----BEGIN CERTIFICATE-----
            MIIB8jCCAXigAwIBAgITBmyf18G7EEwpQ+Vxe3ssyBrBDjAKBggqhkjOPQQDAzA5
            MQswCQYDVQQGEwJVUzEPMA0GA1UEChMGQW1hem9uMRkwFwYDVQQDExBBbWF6b24g
            Um9vdCBDQSA0MB4XDTE1MDUyNjAwMDAwMFoXDTQwMDUyNjAwMDAwMFowOTELMAkG
            A1UEBhMCVVMxDzANBgNVBAoTBkFtYXpvbjEZMBcGA1UEAxMQQW1hem9uIFJvb3Qg
            Q0EgNDB2MBAGByqGSM49AgEGBSuBBAAiA2IABNKrijdPo1MN/sGKe0uoe0ZLY7Bi
            9i0b2whxIdIA6GO9mif78DluXeo9pcmBqqNbIJhFXRbb/egQbeOc4OO9X4Ri83Bk
            M6DLJC9wuoihKqB1+IGuYgbEgds5bimwHvouXKNCMEAwDwYDVR0TAQH/BAUwAwEB
            /zAOBgNVHQ8BAf8EBAMCAYYwHQYDVR0OBBYEFNPsxzplbszh2naaVvuc84ZtV+WB
            MAoGCCqGSM49BAMDA2gAMGUCMDqLIfG9fhGt0O9Yli/W651+kI0rz2ZVwyzjKKlw
            CkcO8DdZEv8tmZQoTipPNU0zWgIxAOp1AE47xDqUEpHJWEadIRNyp4iciuRMStuW
            1KyLa2tJElMzrdfkviT8tQp21KW8EA==
            -----END CERTIFICATE-----
        ";

        assert!(Certificate::from_pem_bundle(PEM_BUNDLE).is_ok())
    }
}
